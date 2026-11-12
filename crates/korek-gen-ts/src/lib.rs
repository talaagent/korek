//! Bare TypeScript: `type`/`interface` declarations and string-literal
//! unions, a `Keys` interface (keys by full dotted name) and
//! `keyDefaults`.
//! No runtime dependency. Validation rules appear as `JSDoc` tags only.
//!
//! The Zod generator builds on this crate ([`declarations`]).
//!
//! Options: `file` (default `types.ts`).

use core::fmt::Write as _;

use korek_gen_common::{GENERATED, js, jsdoc, jsdoc_lines, ts_prop};
use korek_ir::{Diagnostic, Field, RuleKind, Schema, Type, TypeDef, TypeKind, Validate};
use korek_plugin::{File, GenerateRequest, GenerateResponse, Hook, Item};

/// Generator options.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Options {
    pub file: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            file: "types.ts".into(),
        }
    }
}

/// The plugin entry point (`korek-gen-ts`).
///
/// # Errors
/// On unknown options.
pub fn plugin(req: &GenerateRequest) -> Result<GenerateResponse, Diagnostic> {
    req.check_options(&["file"])?;
    let mut opts = Options::default();
    if let Some(f) = req.options.get("file") {
        opts.file.clone_from(f);
    }
    Ok(GenerateResponse::new(generate(&req.schema, &opts, &[])))
}

/// The TypeScript type for `ty`.
pub fn ts_type(ty: &Type) -> String {
    match ty {
        Type::Bool => "boolean".into(),
        Type::String | Type::Bytes | Type::Timestamp | Type::Duration => "string".into(),
        Type::I32 | Type::I64 | Type::U32 | Type::U64 | Type::F32 | Type::F64 => "number".into(),
        Type::List(t) => format!("{}[]", ts_type(t)),
        Type::Map(t) => format!("Record<string, {}>", ts_type(t)),
        Type::Named(n) => n.clone(),
    }
}

/// `JSDoc` tags for rules: `@format email`, `@validate min_len 1`.
pub fn rule_tags(v: &Validate) -> Vec<String> {
    fn walk(v: &Validate, prefix: &str, out: &mut Vec<String>) {
        for r in &v.rules {
            let operand = match &r.kind {
                RuleKind::Format { format } => {
                    out.push(format!("@format {prefix}{}", format.name()));
                    continue;
                }
                RuleKind::Const { value }
                | RuleKind::Gt { value }
                | RuleKind::Gte { value }
                | RuleKind::Lt { value }
                | RuleKind::Lte { value }
                | RuleKind::MultipleOf { value } => format!(" {value}"),
                RuleKind::In { values } | RuleKind::NotIn { values } => {
                    format!(" {}", serde_json::Value::Array(values.clone()))
                }
                RuleKind::Len { n }
                | RuleKind::MinLen { n }
                | RuleKind::MaxLen { n }
                | RuleKind::LenBytes { n }
                | RuleKind::MinBytes { n }
                | RuleKind::MaxBytes { n }
                | RuleKind::MinItems { n }
                | RuleKind::MaxItems { n }
                | RuleKind::MinPairs { n }
                | RuleKind::MaxPairs { n } => format!(" {n}"),
                RuleKind::Pattern { regex } => format!(" {regex:?}"),
                RuleKind::Prefix { value }
                | RuleKind::Suffix { value }
                | RuleKind::Contains { value }
                | RuleKind::NotContains { value } => format!(" {value:?}"),
                RuleKind::Within { duration } => format!(" {duration}"),
                RuleKind::Cel { id, .. } => format!(" {id}"),
                _ => String::new(),
            };
            out.push(format!("@validate {prefix}{}{operand}", r.kind.name()));
        }
        for (name, inner) in [
            ("items", &v.items),
            ("keys", &v.keys),
            ("values", &v.values),
        ] {
            if let Some(inner) = inner {
                walk(inner, &format!("{prefix}{name}."), out);
            }
        }
    }
    let mut out = Vec::new();
    walk(v, "", &mut out);
    out
}

fn field_doc(out: &mut String, f: &Field) {
    let mut tags = rule_tags(&f.validate);
    if let Some(d) = &f.default {
        tags.push(format!("@default {}", js(d)));
    }
    if f.attrs.secret {
        tags.push("@secret".into());
    }
    jsdoc(out, "  ", &jsdoc_lines(&f.attrs, &tags));
}

/// Writes one type declaration.
pub fn type_decl(out: &mut String, t: &TypeDef) {
    jsdoc(out, "", &jsdoc_lines(&t.attrs, &rule_tags(&t.validate)));
    let name = &t.name;
    match &t.kind {
        TypeKind::Enum(e) => {
            let vs: Vec<String> = e.variants.iter().map(|v| format!("{:?}", v.name)).collect();
            let _ = writeln!(out, "export type {name} = {};", vs.join(" | "));
        }
        TypeKind::Struct(s) => {
            let _ = writeln!(out, "export interface {name} {{");
            for f in &s.fields {
                field_doc(out, f);
                // Defaults fill absent fields, so they are present after parsing.
                let q = if f.optional && f.default.is_none() {
                    "?"
                } else {
                    ""
                };
                let _ = writeln!(out, "  {}{q}: {};", ts_prop(&f.name), ts_type(&f.ty));
            }
            out.push_str("}\n");
        }
        TypeKind::Union(u) => {
            let _ = writeln!(out, "export type {name} =");
            let n = u.variants.len();
            for (i, v) in u.variants.iter().enumerate() {
                jsdoc(out, "  ", &jsdoc_lines(&v.attrs, &[]));
                let ty = match &u.tag {
                    Some(tag) => format!(
                        "({{ {}: {:?} }} & {})",
                        ts_prop(tag),
                        v.name,
                        ts_type(&v.ty)
                    ),
                    None => ts_type(&v.ty),
                };
                let end = if i + 1 == n { ";" } else { "" };
                let _ = writeln!(out, "  | {ty}{end}");
            }
        }
    }
}

/// The full dotted name of a key: `session.steer.enabled`.
pub fn key_path(group: &str, key: &str) -> String {
    format!("{group}.{key}")
}

/// Writes every type declaration and the `Keys` interface (the part the
/// Zod generator shares).
pub fn declarations(schema: &Schema, out: &mut String, hooks: &[&dyn Hook]) {
    for t in &schema.types {
        out.push('\n');
        type_decl(out, t);
        for h in hooks {
            h.after(Item::Type(t), out);
        }
    }
    if schema.keys.is_empty() {
        return;
    }
    out.push_str("\n/** Every key, by its full dotted name. */\nexport interface Keys {\n");
    for g in &schema.keys {
        let mut section = vec![format!("`{}` keys.", g.name)];
        section.extend(jsdoc_lines(&g.attrs, &[]));
        let _ = writeln!(
            out,
            "  // {}",
            section.join(" ").lines().next().unwrap_or("")
        );
        for k in &g.keys {
            let mut tags = rule_tags(&k.validate);
            if k.attrs.secret {
                tags.push("@secret".into());
            }
            if let Some(d) = k.default.as_ref().filter(|_| !k.attrs.secret) {
                tags.push(format!("@default {}", js(d)));
            }
            jsdoc(out, "  ", &jsdoc_lines(&k.attrs, &tags));
            let _ = writeln!(
                out,
                "  {:?}: {};",
                key_path(&g.name, &k.name),
                ts_type(&k.ty)
            );
        }
        for h in hooks {
            h.after(Item::KeyGroup(g), out);
        }
    }
    out.push_str("}\n");
}

/// Generates the TypeScript file (plus hook files).
pub fn generate(schema: &Schema, opts: &Options, hooks: &[&dyn Hook]) -> Vec<File> {
    let mut out = String::with_capacity(8192);
    let _ = writeln!(out, "// {GENERATED}");
    declarations(schema, &mut out, hooks);
    if !schema.keys.is_empty() {
        out.push_str("\n/** The defaults of the keys that have one. */\nexport const keyDefaults: Partial<Keys> = {\n");
        for g in &schema.keys {
            for k in &g.keys {
                if let Some(d) = &k.default {
                    let _ = writeln!(out, "  {:?}: {},", key_path(&g.name, &k.name), js(d));
                }
            }
        }
        out.push_str("};\n");
    }
    for h in hooks {
        h.footer(schema, &mut out);
    }
    let mut files = vec![File::new(&opts.file, out)];
    for h in hooks {
        files.extend(h.files(schema));
    }
    files
}
