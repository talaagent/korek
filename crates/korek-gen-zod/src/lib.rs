//! Zod (v4): a `<Name>Schema` per type and a `keys` record of
//! `{ schema, default, doc }` by full key name. Rules become Zod checks.
//!
//! Types come from korek's IR, never from `z.infer`: each schema
//! `satisfies z.ZodType<Name>`, with `Name` imported from the bare TS output
//! (`types=import`, the default; `ts_path` names it, default
//! `../ts/types`) or written into this file by the TS generator
//! (`types=inline`).
//!
//! Options: `file` (default `zod.ts`), `import` (where `z` comes from,
//! default `zod`), `types`, `ts_path`.

use core::fmt::Write as _;
use std::collections::HashSet;

use korek_gen_common::{GENERATED, js, jsdoc, jsdoc_lines, ts_prop, unsupported};
use korek_gen_ts::{declarations, key_path};
use korek_ir::{Diagnostic, Format, Rule, RuleKind, Schema, Type, TypeDef, TypeKind, Validate};
use korek_plugin::{File, GenerateRequest, GenerateResponse, Hook, Item};

/// Where the TS types come from.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Types {
    /// `import type { … } from "<path>"`.
    Import(String),
    /// Declared in the Zod file itself.
    Inline,
}

/// Generator options.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Options {
    pub file: String,
    /// Where `z` is imported from.
    pub import: String,
    pub types: Types,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            file: "zod.ts".into(),
            import: "zod".into(),
            types: Types::Import("../ts/types".into()),
        }
    }
}

/// The plugin entry point (`korek-gen-zod`).
///
/// # Errors
/// On unknown or malformed options.
pub fn plugin(req: &GenerateRequest) -> Result<GenerateResponse, Diagnostic> {
    req.check_options(&["file", "import", "types", "ts_path"])?;
    let mut opts = Options::default();
    if let Some(f) = req.options.get("file") {
        opts.file.clone_from(f);
    }
    if let Some(i) = req.options.get("import") {
        opts.import.clone_from(i);
    }
    let path = req
        .options
        .get("ts_path")
        .cloned()
        .unwrap_or_else(|| "../ts/types".into());
    opts.types = match req.options.get("types").map(String::as_str) {
        None | Some("import") => Types::Import(path),
        Some("inline") => Types::Inline,
        Some(other) => {
            return Err(Diagnostic::error(
                korek_ir::codes::INVALID_OPTION,
                format!("option `types` must be import or inline, found `{other}`"),
            ));
        }
    };
    let (files, warnings) = generate(&req.schema, &opts, &[]);
    Ok(GenerateResponse::new(files).with_diagnostics(warnings))
}

/// Types in dependency order (the IR has no cycles).
fn ordered(schema: &Schema) -> Vec<&TypeDef> {
    fn deps(ty: &Type, out: &mut Vec<String>) {
        match ty {
            Type::List(t) | Type::Map(t) => deps(t, out),
            Type::Named(n) => out.push(n.clone()),
            _ => {}
        }
    }
    fn visit<'s>(
        s: &korek_ir::TypeIndex<'s>,
        t: &'s TypeDef,
        done: &mut HashSet<&'s str>,
        out: &mut Vec<&'s TypeDef>,
    ) {
        if !done.insert(&t.name) {
            return;
        }
        let mut names = Vec::new();
        match &t.kind {
            TypeKind::Struct(st) => st.fields.iter().for_each(|f| deps(&f.ty, &mut names)),
            TypeKind::Union(u) => u.variants.iter().for_each(|v| deps(&v.ty, &mut names)),
            TypeKind::Enum(_) => {}
        }
        for n in names {
            if let Some(d) = s.def(&n) {
                visit(s, d, done, out);
            }
        }
        out.push(t);
    }
    let index = schema.index();
    let mut done = HashSet::new();
    let mut out = Vec::new();
    for t in &schema.types {
        visit(&index, t, &mut done, &mut out);
    }
    out
}

struct Gen<'a> {
    schema: &'a Schema,
    warnings: Vec<Diagnostic>,
}

/// `{ error: "…" }` for a rule's custom message, or nothing.
fn err_param(r: &Rule) -> String {
    r.message
        .as_ref()
        .map(|m| {
            format!(
                ", {{ error: {} }}",
                js(&serde_json::Value::from(m.as_str()))
            )
        })
        .unwrap_or_default()
}

fn regex(p: &str) -> String {
    format!("new RegExp({})", js(&serde_json::Value::from(p)))
}

impl Gen<'_> {
    fn warn(&mut self, r: &Rule, why: &str) {
        self.warnings.push(unsupported("zod", r, why));
    }

    /// The base schema of `ty`, honoring a `format` rule that has a Zod
    /// constructor of its own.
    fn base(&mut self, ty: &Type, v: &Validate) -> (String, Option<Format>) {
        let format = v.rules.iter().find_map(|r| match r.kind {
            RuleKind::Format { format } if *ty == Type::String => Some(format),
            _ => None,
        });
        let ctor = format.and_then(|f| {
            Some(match f {
                Format::Email => "z.email()",
                Format::Url => "z.url()",
                Format::Uuid => "z.uuid()",
                Format::Ulid => "z.ulid()",
                Format::Cuid => "z.cuid()",
                Format::Cuid2 => "z.cuid2()",
                Format::Nanoid => "z.nanoid()",
                Format::Datetime => "z.iso.datetime({ offset: true })",
                Format::Date => "z.iso.date()",
                Format::Time => "z.iso.time()",
                Format::Duration => "z.iso.duration()",
                Format::Base64 => "z.base64()",
                Format::Base64url => "z.base64url()",
                Format::Jwt => "z.jwt()",
                Format::E164 => "z.e164()",
                Format::Emoji => "z.emoji()",
                Format::Ipv4 => "z.ipv4()",
                Format::Ipv6 => "z.ipv6()",
                Format::Hostname => "z.hostname()",
                _ => return None,
            })
        });
        if let Some(c) = ctor {
            return (c.to_owned(), format);
        }
        let b = match ty {
            Type::Bool => "z.boolean()".to_owned(),
            Type::String => "z.string()".to_owned(),
            Type::Bytes => "z.base64()".to_owned(),
            Type::I32 => "z.int32()".to_owned(),
            Type::U32 => "z.uint32()".to_owned(),
            Type::I64 => "z.int()".to_owned(),
            Type::U64 => "z.int().nonnegative()".to_owned(),
            Type::F32 => "z.float32()".to_owned(),
            Type::F64 => "z.number()".to_owned(),
            Type::Timestamp => "z.iso.datetime({ offset: true })".to_owned(),
            Type::Duration => "z.string().regex(/^-?\\d+(\\.\\d+)?s$/)".to_owned(),
            Type::Json => "z.json()".to_owned(),
            Type::List(t) => {
                let empty = Validate::default();
                format!(
                    "z.array({})",
                    self.expr(t, v.items.as_deref().unwrap_or(&empty))
                )
            }
            Type::Map(t) => {
                let empty = Validate::default();
                let k = self.expr(&Type::String, v.keys.as_deref().unwrap_or(&empty));
                format!(
                    "z.record({k}, {})",
                    self.expr(t, v.values.as_deref().unwrap_or(&empty))
                )
            }
            Type::Named(n) => format!("{n}Schema"),
        };
        (b, None)
    }

    /// The schema expression for a value of `ty` with rules `v`.
    fn expr(&mut self, ty: &Type, v: &Validate) -> String {
        let (mut e, used) = self.base(ty, v);
        for r in &v.rules {
            let m = err_param(r);
            let refine = |cond: String| format!(".refine((v) => {cond}{m})");
            let ord = |v: &serde_json::Value| match ty {
                Type::Timestamp | Type::Duration => None,
                _ => Some(v.to_string()),
            };
            let time = |x: &str| match ty {
                Type::Timestamp => format!("Date.parse({x}) / 1000"),
                _ => format!("parseFloat({x})"),
            };
            let s = match &r.kind {
                RuleKind::Format { format } if Some(*format) == used => continue,
                RuleKind::Format { format } => match format {
                    Format::Lowercase => format!(".lowercase({})", m.trim_start_matches(", ")),
                    Format::Uppercase => format!(".uppercase({})", m.trim_start_matches(", ")),
                    Format::Tuuid => format!(".regex(/^[0-9a-fA-F]{{32}}$/{m})"),
                    Format::Uri => format!(".regex(/^[a-zA-Z][a-zA-Z0-9+.-]*:[^\\s]*$/{m})"),
                    Format::Ip => refine("/^[\\d.]+$/.test(v) ? z.ipv4().safeParse(v).success : z.ipv6().safeParse(v).success".into()),
                    _ => {
                        self.warn(r, "no Zod check for this format");
                        continue;
                    }
                },
                RuleKind::Const { value } => refine(format!("v === {}", js(value))),
                RuleKind::In { values } => refine(format!("{}.includes(v)", js(&serde_json::Value::Array(values.clone())))),
                RuleKind::NotIn { values } => refine(format!("!{}.includes(v)", js(&serde_json::Value::Array(values.clone())))),
                RuleKind::Len { n } | RuleKind::MinLen { n } | RuleKind::MaxLen { n } if *ty == Type::Bytes => {
                    let op = match r.kind { RuleKind::Len { .. } => "===", RuleKind::MinLen { .. } => ">=", _ => "<=" };
                    refine(format!("atob(v).length {op} {n}"))
                }
                RuleKind::Len { n } | RuleKind::MinLen { n } | RuleKind::MaxLen { n } => {
                    let op = match r.kind { RuleKind::Len { .. } => "===", RuleKind::MinLen { .. } => ">=", _ => "<=" };
                    refine(format!("[...v].length {op} {n}"))
                }
                RuleKind::LenBytes { n } | RuleKind::MinBytes { n } | RuleKind::MaxBytes { n } => {
                    let op = match r.kind { RuleKind::LenBytes { .. } => "===", RuleKind::MinBytes { .. } => ">=", _ => "<=" };
                    refine(format!("new TextEncoder().encode(v).length {op} {n}"))
                }
                RuleKind::Pattern { regex: p } if *ty == Type::String => format!(".regex({}{m})", regex(p)),
                RuleKind::Prefix { value } if *ty == Type::String => format!(".startsWith({}{m})", js(&value.as_str().into())),
                RuleKind::Suffix { value } if *ty == Type::String => format!(".endsWith({}{m})", js(&value.as_str().into())),
                RuleKind::Contains { value } if *ty == Type::String => format!(".includes({}{m})", js(&value.as_str().into())),
                RuleKind::NotContains { value } => refine(format!("!v.includes({})", js(&value.as_str().into()))),
                RuleKind::Pattern { regex: p } => refine(format!("{}.test(atob(v))", regex(p))),
                RuleKind::Prefix { value } => refine(format!("atob(v).startsWith({})", js(&value.as_str().into()))),
                RuleKind::Suffix { value } => refine(format!("atob(v).endsWith({})", js(&value.as_str().into()))),
                RuleKind::Contains { value } => refine(format!("atob(v).includes({})", js(&value.as_str().into()))),
                RuleKind::Gt { value } | RuleKind::Gte { value } | RuleKind::Lt { value } | RuleKind::Lte { value } => {
                    let (method, op) = match r.kind {
                        RuleKind::Gt { .. } => ("gt", ">"),
                        RuleKind::Gte { .. } => ("gte", ">="),
                        RuleKind::Lt { .. } => ("lt", "<"),
                        _ => ("lte", "<="),
                    };
                    match ord(value) {
                        Some(n) => format!(".{method}({n}{m})"),
                        None => refine(format!("{} {op} {}", time("v"), time(&value.to_string()))),
                    }
                }
                RuleKind::MultipleOf { value } => format!(".multipleOf({value}{m})"),
                RuleKind::MinItems { n } => format!(".min({n}{m})"),
                RuleKind::MaxItems { n } => format!(".max({n}{m})"),
                RuleKind::Unique => refine("new Set(v.map((x) => JSON.stringify(x))).size === v.length".into()),
                RuleKind::MinPairs { n } => refine(format!("Object.keys(v).length >= {n}")),
                RuleKind::MaxPairs { n } => refine(format!("Object.keys(v).length <= {n}")),
                RuleKind::Required | RuleKind::DefinedOnly | RuleKind::Finite => continue,
                RuleKind::GtNow => refine(format!("{} > Date.now() / 1000", time("v"))),
                RuleKind::LtNow => refine(format!("{} < Date.now() / 1000", time("v"))),
                RuleKind::Within { duration } => refine(format!("Math.abs({} - Date.now() / 1000) <= {}", time("v"), korek_runtime_duration(duration))),
                RuleKind::Cel { ts: Some(ts), .. } => format!(".refine({ts}{m})"),
                RuleKind::Cel { .. } => {
                    self.warn(r, "a CEL rule needs `ts=\"(v) => …\"` for Zod");
                    continue;
                }
                _ => {
                    self.warn(r, "not supported");
                    continue;
                }
            };
            e.push_str(&s);
        }
        e
    }

    fn type_def(&mut self, out: &mut String, t: &TypeDef) {
        let name = &t.name;
        jsdoc(out, "", &jsdoc_lines(&t.attrs, &[]));
        let mut body = match &t.kind {
            TypeKind::Enum(e) => {
                let vs: Vec<String> = e.variants.iter().map(|v| format!("{:?}", v.name)).collect();
                format!("z.enum([{}])", vs.join(", "))
            }
            TypeKind::Struct(s) => {
                let mut b = String::from("z.object({\n");
                for f in &s.fields {
                    jsdoc(&mut b, "  ", &jsdoc_lines(&f.attrs, &[]));
                    let mut e = self.expr(&f.ty, &f.validate);
                    if let Some(d) = &f.default {
                        let _ = write!(e, ".default({})", js(d));
                    } else if f.optional && !f.validate.has(|k| matches!(k, RuleKind::Required)) {
                        e.push_str(".optional()");
                    }
                    let _ = writeln!(b, "  {}: {e},", ts_prop(&f.name));
                }
                b.push_str("})");
                b
            }
            TypeKind::Union(u) => {
                let mut b = match &u.tag {
                    Some(tag) => format!("z.discriminatedUnion({tag:?}, [\n"),
                    None => "z.union([\n".to_owned(),
                };
                for v in &u.variants {
                    let e = self.expr(&v.ty, &Validate::default());
                    match &u.tag {
                        Some(tag) => {
                            let _ = writeln!(
                                b,
                                "  {e}.extend({{ {}: z.literal({:?}) }}),",
                                ts_prop(tag),
                                v.name
                            );
                        }
                        None => {
                            let _ = writeln!(b, "  {e},");
                        }
                    }
                }
                b.push_str("])");
                b
            }
        };
        for r in &t.validate.rules {
            match &r.kind {
                RuleKind::Cel { ts: Some(ts), .. } => {
                    let _ = write!(body, ".refine({ts}{})", err_param(r));
                }
                RuleKind::Cel { .. } => self.warn(r, "a CEL rule needs `ts=\"(v) => …\"` for Zod"),
                _ => {}
            }
        }
        let _ = writeln!(
            out,
            "export const {name}Schema = {body} satisfies z.ZodType<{name}>;"
        );
    }
}

/// Seconds of a `"3600s"` duration, for the generated JS.
fn korek_runtime_duration(d: &str) -> String {
    d.trim_end_matches('s').to_owned()
}

/// Generates the Zod file (plus hook files) and warnings for rules Zod
/// cannot express.
pub fn generate(
    schema: &Schema,
    opts: &Options,
    hooks: &[&dyn Hook],
) -> (Vec<File>, Vec<Diagnostic>) {
    let mut out = String::with_capacity(16 * 1024);
    let _ = writeln!(
        out,
        "// {GENERATED}\nimport {{ z }} from {:?};",
        opts.import
    );
    match &opts.types {
        Types::Import(path) => {
            let mut names: Vec<&str> = schema.types.iter().map(|t| t.name.as_str()).collect();
            if !schema.keys.is_empty() {
                names.push("Keys");
            }
            if !names.is_empty() {
                let _ = writeln!(out, "import type {{ {} }} from {path:?};", names.join(", "));
                let _ = writeln!(out, "export type {{ {} }};", names.join(", "));
            }
        }
        Types::Inline => declarations(schema, &mut out, &[]),
    }
    let mut g = Gen {
        schema,
        warnings: Vec::new(),
    };
    for t in ordered(schema) {
        out.push('\n');
        g.type_def(&mut out, t);
        for h in hooks {
            h.after(Item::Type(t), &mut out);
        }
    }
    if !schema.keys.is_empty() {
        out.push_str(
            "\n/** A key: its schema, default (if any) and doc. */\nexport interface Key<T> {\n  schema: z.ZodType<T>;\n  default?: T;\n  doc: string;\n  secret?: boolean;\n}\n\n/** Every key, by its full dotted name. */\nexport const keys: { [K in keyof Keys]-?: Key<Keys[K]> } = {\n",
        );
        for grp in &schema.keys {
            for k in &grp.keys {
                jsdoc(&mut out, "  ", &jsdoc_lines(&k.attrs, &[]));
                let schema_expr = g.expr(&k.ty, &k.validate);
                let _ = write!(
                    out,
                    "  {:?}: {{\n    schema: {schema_expr},\n",
                    key_path(&grp.name, &k.name)
                );
                if let Some(d) = &k.default {
                    let _ = writeln!(out, "    default: {},", js(d));
                }
                let doc = korek_gen_common::doc_text(&k.attrs).unwrap_or_default();
                let _ = writeln!(out, "    doc: {},", js(&doc.into()));
                if k.attrs.secret {
                    out.push_str("    secret: true,\n");
                }
                out.push_str("  },\n");
                for h in hooks {
                    h.after(Item::Key(grp, k), &mut out);
                }
            }
        }
        out.push_str("};\n");
    }
    let _ = g.schema;
    for h in hooks {
        h.footer(schema, &mut out);
    }
    let mut files = vec![File::new(&opts.file, out)];
    for h in hooks {
        files.extend(h.files(schema));
    }
    (files, g.warnings)
}
