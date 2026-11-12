//! Rust: serde types for every named type, and a module of typed key
//! consts (`pub const STEER_ENABLED: Key<bool>`) per keys block.
//!
//! The output is one file, `types.rs` by default (option `file`), with no
//! `use` items and no inner attributes, so it can be `include!`d anywhere.
//! It needs `serde` (with `derive`) in the including crate.

use core::fmt::Write as _;

use heck::{ToShoutySnakeCase as _, ToSnakeCase as _, ToUpperCamelCase as _};
use korek_core::Diagnostic;
use korek_core::ir::{
    Constraints, EnumDef, KeyGroup, Schema, StructDef, Type, TypeDef, TypeKind, UnionDef,
    format_number,
};
use korek_core::value::{Value, resolve};
use korek_plugin::text::{GENERATED, doc_lines};
use korek_plugin::{File, GenerateRequest, GenerateResponse, Hook, Item};

/// Generator options.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Options {
    /// The output file name.
    pub file: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            file: "types.rs".into(),
        }
    }
}

/// The plugin entry point (`korek-gen-rust`).
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

/// Generates the Rust file (plus any files the hooks add).
pub fn generate(schema: &Schema, opts: &Options, hooks: &[&dyn Hook]) -> Vec<File> {
    let mut out = format!("// {GENERATED}\n");
    if !schema.keys.is_empty() {
        out.push_str(KEY_TYPES);
    }
    for t in &schema.types {
        out.push('\n');
        type_def(&mut out, t);
        for h in hooks {
            h.after(Item::Type(t), &mut out);
        }
    }
    for g in &schema.keys {
        out.push('\n');
        key_group(&mut out, schema, g, hooks);
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

const KEY_TYPES: &str = r#"
/// A typed key: its name, documentation, default and constraints.
pub struct Key<T> {
    /// The keys block, e.g. `session`.
    pub group: &'static str,
    /// The key within its block, e.g. `steer.enabled`.
    pub name: &'static str,
    pub doc: &'static str,
    /// Builds the default value.
    pub default: Option<fn() -> T>,
    /// The default as JSON.
    pub default_json: Option<&'static str>,
    pub constraints: &'static [Constraint],
}

impl<T> Key<T> {
    /// `group.name`.
    pub fn path(&self) -> String {
        format!("{}.{}", self.group, self.name)
    }

    /// The default value, if the key has one.
    pub fn default_value(&self) -> Option<T> {
        self.default.map(|f| f())
    }
}

impl<T> Clone for Key<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Key<T> {}

impl<T> ::core::fmt::Debug for Key<T> {
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        f.debug_struct("Key")
            .field("group", &self.group)
            .field("name", &self.name)
            .field("default_json", &self.default_json)
            .field("constraints", &self.constraints)
            .finish_non_exhaustive()
    }
}

/// A value constraint on a key or field. Bounds are inclusive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Constraint {
    Min(f64),
    Max(f64),
    MinLen(u64),
    MaxLen(u64),
    /// A regular expression the value must match.
    Pattern(&'static str),
}
"#;

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern", "false",
    "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "static", "struct", "trait", "true", "type", "unsafe", "use", "where",
    "while", "abstract", "become", "box", "do", "final", "macro", "override", "priv", "try",
    "typeof", "unsized", "virtual", "yield",
];

/// A field or module name, raw when it is a keyword.
fn ident(name: &str) -> String {
    if KEYWORDS.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_owned()
    }
}

/// The Rust type for `ty`.
pub fn rust_type(ty: &Type) -> String {
    match ty {
        Type::Bool => "bool".into(),
        Type::String => "String".into(),
        Type::I64 => "i64".into(),
        Type::U64 => "u64".into(),
        Type::F64 => "f64".into(),
        Type::Bytes => "Vec<u8>".into(),
        Type::List(t) => format!("Vec<{}>", rust_type(t)),
        Type::Map(t) => format!("::std::collections::BTreeMap<String, {}>", rust_type(t)),
        Type::Named(n) => n.clone(),
        _ => "()".into(),
    }
}

fn variant_ident(name: &str) -> String {
    name.to_upper_camel_case()
}

/// `#[serde(rename = …)]` when `rename_all = "snake_case"` would not give
/// back `name`.
fn rename(out: &mut String, indent: &str, name: &str) {
    if variant_ident(name).to_snake_case() != name {
        let _ = writeln!(out, "{indent}#[serde(rename = {name:?})]");
    }
}

fn type_def(out: &mut String, t: &TypeDef) {
    doc_lines(out, "", "/// ", t.doc.as_deref());
    match &t.kind {
        TypeKind::Enum(e) => enum_def(out, &t.name, e),
        TypeKind::Struct(s) => struct_def(out, &t.name, s),
        TypeKind::Union(u) => union_def(out, &t.name, u),
        _ => {}
    }
}

const SERDE: &str = "::serde::Serialize, ::serde::Deserialize";

fn enum_def(out: &mut String, name: &str, e: &EnumDef) {
    let _ = writeln!(
        out,
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, {SERDE})]\n\
         #[serde(rename_all = \"snake_case\")]\npub enum {name} {{"
    );
    for v in &e.variants {
        doc_lines(out, "    ", "/// ", v.doc.as_deref());
        rename(out, "    ", &v.name);
        let _ = writeln!(out, "    {},", variant_ident(&v.name));
    }
    out.push_str("}\n");
}

fn struct_def(out: &mut String, name: &str, s: &StructDef) {
    let _ = writeln!(
        out,
        "#[derive(Debug, Clone, PartialEq, {SERDE})]\npub struct {name} {{"
    );
    for f in &s.fields {
        doc_lines(out, "    ", "/// ", f.doc.as_deref());
        constraint_doc(out, "    ", f.doc.is_some(), &f.constraints);
        let ty = rust_type(&f.ty);
        if f.optional {
            out.push_str("    #[serde(default, skip_serializing_if = \"Option::is_none\")]\n");
            let _ = writeln!(out, "    pub {}: Option<{ty}>,", ident(&f.name));
        } else {
            let _ = writeln!(out, "    pub {}: {ty},", ident(&f.name));
        }
    }
    out.push_str("}\n");
}

fn union_def(out: &mut String, name: &str, u: &UnionDef) {
    let _ = writeln!(out, "#[derive(Debug, Clone, PartialEq, {SERDE})]");
    match &u.tag {
        Some(tag) => {
            let _ = writeln!(out, "#[serde(tag = {tag:?}, rename_all = \"snake_case\")]");
        }
        None => out.push_str("#[serde(untagged)]\n"),
    }
    let _ = writeln!(out, "pub enum {name} {{");
    for v in &u.variants {
        doc_lines(out, "    ", "/// ", v.doc.as_deref());
        if u.tag.is_some() {
            rename(out, "    ", &v.name);
        }
        let _ = writeln!(out, "    {}({}),", variant_ident(&v.name), rust_type(&v.ty));
    }
    out.push_str("}\n");
}

/// Documents constraints as a doc paragraph (Rust has no native form).
fn constraint_doc(out: &mut String, indent: &str, after_doc: bool, c: &Constraints) {
    let list = constraint_list(c);
    if list.is_empty() {
        return;
    }
    if after_doc {
        let _ = writeln!(out, "{indent}///");
    }
    let _ = writeln!(out, "{indent}/// Constraints: {}.", list.join(", "));
}

fn constraint_list(c: &Constraints) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(n) = c.min {
        v.push(format!("min {}", format_number(n)));
    }
    if let Some(n) = c.max {
        v.push(format!("max {}", format_number(n)));
    }
    if let Some(n) = c.min_len {
        v.push(format!("min_len {n}"));
    }
    if let Some(n) = c.max_len {
        v.push(format!("max_len {n}"));
    }
    if let Some(p) = &c.pattern {
        v.push(format!("pattern `{p}`"));
    }
    v
}

fn constraint_consts(c: &Constraints) -> String {
    let mut v = Vec::new();
    if let Some(n) = c.min {
        v.push(format!("super::Constraint::Min({n:?})"));
    }
    if let Some(n) = c.max {
        v.push(format!("super::Constraint::Max({n:?})"));
    }
    if let Some(n) = c.min_len {
        v.push(format!("super::Constraint::MinLen({n})"));
    }
    if let Some(n) = c.max_len {
        v.push(format!("super::Constraint::MaxLen({n})"));
    }
    if let Some(p) = &c.pattern {
        v.push(format!("super::Constraint::Pattern({p:?})"));
    }
    format!("&[{}]", v.join(", "))
}

fn key_group(out: &mut String, schema: &Schema, g: &KeyGroup, hooks: &[&dyn Hook]) {
    doc_lines(out, "", "/// ", g.doc.as_deref());
    let _ = writeln!(
        out,
        "pub mod {} {{\n    #[allow(unused_imports)]\n    use super::*;\n",
        ident(&g.ident())
    );
    for k in &g.keys {
        doc_lines(out, "    ", "/// ", k.doc.as_deref());
        let ty = rust_type(&k.ty);
        let (default, default_json) = match &k.default {
            Some(j) => {
                let v = resolve(schema, &k.ty, j, &Constraints::default());
                let lit = v.map_or_else(|_| "unreachable!()".into(), |v| literal(schema, &v));
                (
                    format!("Some(|| {lit})"),
                    format!("Some({:?})", j.to_string()),
                )
            }
            None => ("None".into(), "None".into()),
        };
        let _ = writeln!(
            out,
            "    pub const {}: super::Key<{ty}> = super::Key {{\n        \
             group: {:?},\n        name: {:?},\n        doc: {:?},\n        \
             default: {default},\n        default_json: {default_json},\n        \
             constraints: {},\n    }};",
            k.ident().to_shouty_snake_case(),
            g.name,
            k.name,
            k.doc.as_deref().unwrap_or(""),
            constraint_consts(&k.constraints),
        );
        for h in hooks {
            h.after(Item::Key(g, k), out);
        }
        out.push('\n');
    }
    let names: Vec<String> = g.keys.iter().map(|k| format!("{:?}", k.name)).collect();
    let _ = writeln!(
        out,
        "    /// Every key in this block.\n    pub const NAMES: &[&str] = &[{}];",
        names.join(", ")
    );
    for h in hooks {
        h.after(Item::KeyGroup(g), out);
    }
    out.push_str("}\n");
}

/// A Rust expression building `v`.
pub fn literal(schema: &Schema, v: &Value) -> String {
    let lit = |v: &Value| literal(schema, v);
    match v {
        Value::Bool(b) => b.to_string(),
        Value::String(s) => format!("String::from({s:?})"),
        Value::I64(i) => i.to_string(),
        Value::U64(u) => u.to_string(),
        Value::F64(f) => format!("{f:?}"),
        Value::Bytes(b) => format!("vec!{b:?}"),
        Value::List(l) => {
            let items: Vec<String> = l.iter().map(lit).collect();
            format!("vec![{}]", items.join(", "))
        }
        Value::Map(m) if m.is_empty() => "::std::collections::BTreeMap::new()".into(),
        Value::Map(m) => {
            let items: Vec<String> = m
                .iter()
                .map(|(k, v)| format!("(String::from({k:?}), {})", lit(v)))
                .collect();
            format!("::std::collections::BTreeMap::from([{}])", items.join(", "))
        }
        Value::Enum { ty, variant } => format!("{ty}::{}", variant_ident(variant)),
        Value::Struct { ty, fields } => {
            let optional = |name: &str| match schema.get(ty).map(|t| &t.kind) {
                Some(TypeKind::Struct(s)) => s.fields.iter().any(|f| f.name == name && f.optional),
                _ => false,
            };
            let fs: Vec<String> = fields
                .iter()
                .map(|(k, v)| {
                    let v = match v {
                        None => "None".into(),
                        Some(v) if optional(k) => format!("Some({})", lit(v)),
                        Some(v) => lit(v),
                    };
                    format!("{}: {v}", ident(k))
                })
                .collect();
            format!("{ty} {{ {} }}", fs.join(", "))
        }
        Value::Union { ty, variant, value } => {
            format!("{ty}::{}({})", variant_ident(variant), lit(value))
        }
        _ => "unreachable!()".into(),
    }
}
