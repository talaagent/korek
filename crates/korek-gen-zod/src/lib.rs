//! TypeScript Zod (v4): a `<Name>Schema` and an inferred `<Name>` type per
//! named type, and a `keys` object of `{ schema, default, doc }` per keys
//! block. Constraints become Zod checks.
//!
//! Options: `file` (default `zod.ts`), `import` (the module to import `z`
//! from, default `zod`).

use core::fmt::Write as _;
use std::collections::HashSet;

use korek_core::Diagnostic;
use korek_core::ir::{Constraints, Schema, Type, TypeDef, TypeKind, format_number};
use korek_core::naming;
use korek_gen_ts::{jsdoc, prop};
use korek_plugin::text::GENERATED;
use korek_plugin::{File, GenerateRequest, GenerateResponse, Hook, Item};

/// Generator options.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Options {
    pub file: String,
    /// Where `z` is imported from.
    pub import: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            file: "zod.ts".into(),
            import: "zod".into(),
        }
    }
}

/// The plugin entry point (`korek-gen-zod`).
///
/// # Errors
/// On unknown options.
pub fn plugin(req: &GenerateRequest) -> Result<GenerateResponse, Diagnostic> {
    req.check_options(&["file", "import"])?;
    let mut opts = Options::default();
    if let Some(f) = req.options.get("file") {
        opts.file.clone_from(f);
    }
    if let Some(i) = req.options.get("import") {
        opts.import.clone_from(i);
    }
    Ok(GenerateResponse::new(generate(&req.schema, &opts, &[])))
}

/// Types in dependency order (the IR has no cycles), declaration order
/// otherwise.
fn ordered(schema: &Schema) -> Vec<&TypeDef> {
    fn deps(ty: &Type, out: &mut Vec<String>) {
        match ty {
            Type::List(t) | Type::Map(t) => deps(t, out),
            Type::Named(n) => out.push(n.clone()),
            _ => {}
        }
    }
    fn visit<'s>(s: &'s Schema, t: &'s TypeDef, done: &mut HashSet<&'s str>, out: &mut Vec<&'s TypeDef>) {
        if !done.insert(&t.name) {
            return;
        }
        let mut names = Vec::new();
        match &t.kind {
            TypeKind::Struct(st) => st.fields.iter().for_each(|f| deps(&f.ty, &mut names)),
            TypeKind::Union(u) => u.variants.iter().for_each(|v| deps(&v.ty, &mut names)),
            _ => {}
        }
        for n in names {
            if let Some(d) = s.get(&n) {
                visit(s, d, done, out);
            }
        }
        out.push(t);
    }
    let mut done = HashSet::new();
    let mut out = Vec::new();
    for t in &schema.types {
        visit(schema, t, &mut done, &mut out);
    }
    out
}

/// The Zod expression for `ty` with constraints `c`.
pub fn zod(ty: &Type, c: &Constraints) -> String {
    let mut e = match ty {
        Type::Bool => "z.boolean()".to_owned(),
        Type::String => "z.string()".to_owned(),
        Type::I64 => "z.number().int()".to_owned(),
        Type::U64 => "z.number().int().nonnegative()".to_owned(),
        Type::F64 => "z.number()".to_owned(),
        Type::Bytes => "z.array(z.number().int().min(0).max(255))".to_owned(),
        Type::List(t) => format!("z.array({})", zod(t, &Constraints::default())),
        Type::Map(t) => format!("z.record(z.string(), {})", zod(t, &Constraints::default())),
        Type::Named(n) => format!("{n}Schema"),
        _ => "z.unknown()".to_owned(),
    };
    if let Some(n) = c.min {
        let _ = write!(e, ".min({})", format_number(n));
    }
    if let Some(n) = c.max {
        let _ = write!(e, ".max({})", format_number(n));
    }
    let is_map = matches!(ty, Type::Map(_));
    if let Some(n) = c.min_len {
        if is_map {
            let _ = write!(
                e,
                ".refine((m) => Object.keys(m).length >= {n}, {{ message: \"needs at least {n} entries\" }})"
            );
        } else {
            let _ = write!(e, ".min({n})");
        }
    }
    if let Some(n) = c.max_len {
        if is_map {
            let _ = write!(
                e,
                ".refine((m) => Object.keys(m).length <= {n}, {{ message: \"allows at most {n} entries\" }})"
            );
        } else {
            let _ = write!(e, ".max({n})");
        }
    }
    if let Some(p) = &c.pattern {
        let _ = write!(e, ".regex(new RegExp({}))", serde_json::Value::from(p.as_str()));
    }
    e
}

/// Types a key entry so its default is checked against its schema.
const KEY_HELPER: &str = "
/** A key: its schema, its default (checked against the schema) and doc. */
export interface Key<S extends z.ZodType> {
  schema: S;
  default?: z.infer<S>;
  doc: string;
}

function key<S extends z.ZodType>(k: Key<S>): Key<S> {
  return k;
}
";

/// Generates the Zod file (plus hook files).
pub fn generate(schema: &Schema, opts: &Options, hooks: &[&dyn Hook]) -> Vec<File> {
    let mut out = format!("// {GENERATED}\nimport {{ z }} from {:?};\n", opts.import);
    for t in ordered(schema) {
        out.push('\n');
        type_def(schema, &mut out, t);
        for h in hooks {
            h.after(Item::Type(t), &mut out);
        }
    }
    if !schema.keys.is_empty() {
        out.push_str(KEY_HELPER);
        out.push_str("\n/** Every keys block: each key's schema, default and doc. */\nexport const keys = {\n");
        for g in &schema.keys {
            jsdoc(&mut out, "  ", g.doc.as_deref());
            let _ = writeln!(out, "  {}: {{", prop(&g.name));
            for k in &g.keys {
                jsdoc(&mut out, "    ", k.doc.as_deref());
                let default = k
                    .default
                    .as_ref()
                    .map(|d| format!("      default: {d},\n"))
                    .unwrap_or_default();
                let doc = serde_json::Value::from(k.doc.as_deref().unwrap_or(""));
                let _ = writeln!(
                    out,
                    "    {:?}: key({{\n      schema: {},\n{default}      doc: {doc},\n    }}),",
                    k.name,
                    zod(&k.ty, &k.constraints)
                );
                for h in hooks {
                    h.after(Item::Key(g, k), &mut out);
                }
            }
            out.push_str("  },\n");
        }
        out.push_str("};\n");
        for g in &schema.keys {
            let _ = writeln!(
                out,
                "\n/** The values of the `{0}` keys. */\nexport type {1} = {{\n  [K in keyof (typeof keys)[{0:?}]]: z.infer<(typeof keys)[{0:?}][K][\"schema\"]>;\n}};",
                g.name,
                naming::keys_type(g)
            );
            for h in hooks {
                h.after(Item::KeyGroup(g), &mut out);
            }
        }
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

fn type_def(schema: &Schema, out: &mut String, t: &TypeDef) {
    let name = &t.name;
    jsdoc(out, "", t.doc.as_deref());
    match &t.kind {
        TypeKind::Enum(e) => {
            let vs: Vec<String> = e.variants.iter().map(|v| format!("{:?}", v.name)).collect();
            let _ = writeln!(out, "export const {name}Schema = z.enum([{}]);", vs.join(", "));
        }
        TypeKind::Struct(s) => {
            let _ = writeln!(out, "export const {name}Schema = z.object({{");
            for f in &s.fields {
                jsdoc(out, "  ", f.doc.as_deref());
                let opt = if f.optional { ".optional()" } else { "" };
                let _ = writeln!(out, "  {}: {}{opt},", prop(&f.name), zod(&f.ty, &f.constraints));
            }
            out.push_str("});\n");
        }
        TypeKind::Union(u) => {
            match &u.tag {
                Some(tag) => {
                    let _ = writeln!(out, "export const {name}Schema = z.discriminatedUnion({tag:?}, [");
                    for v in &u.variants {
                        jsdoc(out, "  ", v.doc.as_deref());
                        let _ = writeln!(
                            out,
                            "  {}.extend({{ {}: z.literal({:?}) }}),",
                            zod(&v.ty, &Constraints::default()),
                            prop(tag),
                            v.name
                        );
                    }
                }
                None => {
                    let _ = writeln!(out, "export const {name}Schema = z.union([");
                    for v in &u.variants {
                        jsdoc(out, "  ", v.doc.as_deref());
                        let _ = writeln!(out, "  {},", zod(&v.ty, &Constraints::default()));
                    }
                }
            }
            out.push_str("]);\n");
        }
        _ => {}
    }
    let _ = schema;
    let _ = writeln!(out, "export type {name} = z.infer<typeof {name}Schema>;");
}
