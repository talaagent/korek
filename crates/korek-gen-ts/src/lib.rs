//! Bare TypeScript: `type`/`interface` declarations and string-literal
//! unions, plus an interface and a defaults object per keys block. No
//! runtime dependency.
//!
//! Options: `file` (default `types.ts`).

use core::fmt::Write as _;

use heck::ToLowerCamelCase as _;
use korek_core::Diagnostic;
use korek_core::ir::{Schema, Type, TypeDef, TypeKind};
use korek_core::naming;
use korek_plugin::text::GENERATED;
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

/// Writes a JSDoc comment.
pub fn jsdoc(out: &mut String, indent: &str, doc: Option<&str>) {
    let Some(doc) = doc else { return };
    let lines: Vec<&str> = doc.lines().collect();
    if let [one] = lines.as_slice() {
        let _ = writeln!(out, "{indent}/** {} */", one.replace("*/", "*\\/"));
        return;
    }
    let _ = writeln!(out, "{indent}/**");
    for l in lines {
        let l = l.replace("*/", "*\\/");
        if l.is_empty() {
            let _ = writeln!(out, "{indent} *");
        } else {
            let _ = writeln!(out, "{indent} * {l}");
        }
    }
    let _ = writeln!(out, "{indent} */");
}

/// A property name: bare when it is an identifier, else quoted.
pub fn prop(name: &str) -> String {
    let ident = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ident {
        name.to_owned()
    } else {
        format!("{name:?}")
    }
}

/// The TypeScript type for `ty`.
pub fn ts_type(ty: &Type) -> String {
    match ty {
        Type::Bool => "boolean".into(),
        Type::String => "string".into(),
        Type::I64 | Type::U64 | Type::F64 => "number".into(),
        Type::Bytes => "number[]".into(),
        Type::List(t) => format!("{}[]", ts_type(t)),
        Type::Map(t) => format!("Record<string, {}>", ts_type(t)),
        Type::Named(n) => n.clone(),
        _ => "unknown".into(),
    }
}

/// Generates the TypeScript file (plus hook files).
pub fn generate(schema: &Schema, opts: &Options, hooks: &[&dyn Hook]) -> Vec<File> {
    let mut out = format!("// {GENERATED}\n");
    for t in &schema.types {
        out.push('\n');
        type_def(&mut out, t);
        for h in hooks {
            h.after(Item::Type(t), &mut out);
        }
    }
    for g in &schema.keys {
        let name = naming::keys_type(g);
        out.push('\n');
        jsdoc(&mut out, "", g.doc.as_deref());
        let _ = writeln!(out, "export interface {name} {{");
        for k in &g.keys {
            jsdoc(&mut out, "  ", k.doc.as_deref());
            let _ = writeln!(out, "  {}: {};", prop(&k.name), ts_type(&k.ty));
        }
        out.push_str("}\n");
        let _ = writeln!(
            out,
            "\n/** The defaults of the `{}` keys that have one. */\nexport const {}Defaults: Partial<{name}> = {{",
            g.name,
            g.ident().to_lower_camel_case()
        );
        for k in &g.keys {
            if let Some(d) = &k.default {
                let _ = writeln!(out, "  {}: {d},", prop(&k.name));
            }
        }
        out.push_str("};\n");
        for h in hooks {
            h.after(Item::KeyGroup(g), &mut out);
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

fn type_def(out: &mut String, t: &TypeDef) {
    jsdoc(out, "", t.doc.as_deref());
    let name = &t.name;
    match &t.kind {
        TypeKind::Enum(e) => {
            let vs: Vec<String> = e.variants.iter().map(|v| format!("{:?}", v.name)).collect();
            let _ = writeln!(out, "export type {name} = {};", vs.join(" | "));
        }
        TypeKind::Struct(s) => {
            let _ = writeln!(out, "export interface {name} {{");
            for f in &s.fields {
                jsdoc(out, "  ", f.doc.as_deref());
                let q = if f.optional { "?" } else { "" };
                let _ = writeln!(out, "  {}{q}: {};", prop(&f.name), ts_type(&f.ty));
            }
            out.push_str("}\n");
        }
        TypeKind::Union(u) => {
            let _ = writeln!(out, "export type {name} =");
            let n = u.variants.len();
            for (i, v) in u.variants.iter().enumerate() {
                jsdoc(out, "  ", v.doc.as_deref());
                let ty = match &u.tag {
                    Some(tag) => format!("({{ {}: {:?} }} & {})", prop(tag), v.name, ts_type(&v.ty)),
                    None => ts_type(&v.ty),
                };
                let end = if i + 1 == n { ";" } else { "" };
                let _ = writeln!(out, "  | {ty}{end}");
            }
        }
        _ => {}
    }
}
