//! Rust: serde types, a `korek_runtime::Validate` impl per type, `Default`
//! impls where every field has a default, and a `keys` module of typed key
//! consts (`pub const SESSION_STEER_ENABLED: Key<bool>`).
//!
//! The output is one file, `types.rs` by default (option `file`), with no
//! `use` items and no inner attributes, so it can be `include!`d. It needs
//! `serde` (with `derive`) and `korek-runtime` in the including crate.

mod validate;

use core::fmt::Write as _;

use korek_gen_common::{GENERATED, comment, doc_text, rust_ident};
use korek_ir::naming::{pascal, shouty};
use korek_ir::value::{Value, resolve};
use korek_ir::{Diagnostic, Field, Key, KeyGroup, Schema, Type, TypeDef, TypeKind};
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
    let (files, warnings) = generate(&req.schema, &opts, &[]);
    Ok(GenerateResponse::new(files).with_diagnostics(warnings))
}

pub(crate) const RT: &str = "::korek_runtime";
const SERDE: &str = "::serde::Serialize, ::serde::Deserialize";

/// The Rust type for `ty`.
pub fn rust_type(ty: &Type) -> String {
    match ty {
        Type::Bool => "bool".into(),
        Type::String | Type::Timestamp | Type::Duration => "String".into(),
        Type::Bytes => "Vec<u8>".into(),
        Type::I32 => "i32".into(),
        Type::I64 => "i64".into(),
        Type::U32 => "u32".into(),
        Type::U64 => "u64".into(),
        Type::F32 => "f32".into(),
        Type::F64 => "f64".into(),
        Type::List(t) => format!("Vec<{}>", rust_type(t)),
        Type::Map(t) => format!("::std::collections::BTreeMap<String, {}>", rust_type(t)),
        Type::Named(n) => n.clone(),
    }
}

/// Doc comments for an item, with examples.
fn docs(out: &mut String, indent: &str, attrs: &korek_ir::Attrs, extra: &[String]) {
    let mut text = doc_text(attrs).unwrap_or_default();
    for e in extra {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(e);
    }
    if !attrs.examples.is_empty() && !attrs.secret {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str("Examples:\n");
        for e in &attrs.examples {
            let _ = write!(text, "\n- `{e}`");
        }
    }
    comment(
        out,
        indent,
        "/// ",
        (!text.is_empty()).then_some(text.as_str()),
    );
    if let Some(d) = &attrs.deprecated {
        let _ = writeln!(out, "{indent}#[deprecated(note = {d:?})]");
    }
}

fn rules_doc(v: &korek_ir::Validate) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for r in &v.rules {
        tags.push(format!("`{}`", r.kind.name()));
    }
    if tags.is_empty() {
        Vec::new()
    } else {
        vec![format!("Validated: {}.", tags.join(", "))]
    }
}

struct Gen<'a> {
    schema: &'a Schema,
    out: String,
    warnings: Vec<Diagnostic>,
}

/// Generates the Rust file (plus hook files) and warnings for rules it
/// cannot express.
pub fn generate(
    schema: &Schema,
    opts: &Options,
    hooks: &[&dyn Hook],
) -> (Vec<File>, Vec<Diagnostic>) {
    let mut g = Gen {
        schema,
        out: String::with_capacity(32 * 1024),
        warnings: Vec::new(),
    };
    let _ = writeln!(g.out, "// {GENERATED}");
    for t in &schema.types {
        g.out.push('\n');
        g.type_def(t);
        for h in hooks {
            h.after(Item::Type(t), &mut g.out);
        }
    }
    if !schema.keys.is_empty() {
        g.keys(hooks);
    }
    for h in hooks {
        h.footer(schema, &mut g.out);
    }
    let mut files = vec![File::new(&opts.file, g.out)];
    for h in hooks {
        files.extend(h.files(schema));
    }
    (files, g.warnings)
}

impl Gen<'_> {
    fn type_def(&mut self, t: &TypeDef) {
        docs(&mut self.out, "", &t.attrs, &[]);
        match &t.kind {
            TypeKind::Enum(e) => {
                let _ = writeln!(
                    self.out,
                    "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, {SERDE})]\npub enum {} {{",
                    t.name
                );
                for v in &e.variants {
                    docs(&mut self.out, "    ", &v.attrs, &[]);
                    let _ = writeln!(
                        self.out,
                        "    #[serde(rename = {:?})]\n    {},",
                        v.name,
                        pascal(&v.name)
                    );
                }
                self.out.push_str("}\n");
            }
            TypeKind::Struct(s) => self.struct_def(t, &s.fields),
            TypeKind::Union(u) => {
                let _ = writeln!(self.out, "#[derive(Debug, Clone, PartialEq, {SERDE})]");
                match &u.tag {
                    Some(tag) => {
                        let _ = writeln!(self.out, "#[serde(tag = {tag:?})]");
                    }
                    None => self.out.push_str("#[serde(untagged)]\n"),
                }
                let _ = writeln!(self.out, "pub enum {} {{", t.name);
                for v in &u.variants {
                    docs(&mut self.out, "    ", &v.attrs, &[]);
                    if u.tag.is_some() {
                        let _ = writeln!(self.out, "    #[serde(rename = {:?})]", v.name);
                    }
                    let _ = writeln!(self.out, "    {}({}),", pascal(&v.name), rust_type(&v.ty));
                }
                self.out.push_str("}\n");
            }
        }
        self.validate_impl(t);
    }

    /// The field's Rust type: `Option<T>` when optional without a default.
    fn field_type(f: &Field) -> String {
        let ty = rust_type(&f.ty);
        if f.optional && f.default.is_none() {
            format!("Option<{ty}>")
        } else {
            ty
        }
    }

    fn struct_def(&mut self, t: &TypeDef, fields: &[Field]) {
        let _ = writeln!(
            self.out,
            "#[derive(Debug, Clone, PartialEq, {SERDE})]\npub struct {} {{",
            t.name
        );
        for f in fields {
            docs(&mut self.out, "    ", &f.attrs, &rules_doc(&f.validate));
            let opt = f.optional && f.default.is_none();
            let mut serde = Vec::new();
            if f.ty == Type::Bytes {
                serde.push(format!(
                    "with = \"{RT}::base64{}\"",
                    if opt { "::option" } else { "" }
                ));
            }
            if opt {
                serde.push("default".into());
                serde.push("skip_serializing_if = \"Option::is_none\"".into());
            }
            if f.default.is_some() {
                serde.push(format!("default = \"{}::default_{}\"", t.name, f.name));
            }
            if !serde.is_empty() {
                let _ = writeln!(self.out, "    #[serde({})]", serde.join(", "));
            }
            let _ = writeln!(
                self.out,
                "    pub {}: {},",
                rust_ident(&f.name),
                Self::field_type(f)
            );
        }
        self.out.push_str("}\n");
        let defaults: Vec<&Field> = fields.iter().filter(|f| f.default.is_some()).collect();
        if !defaults.is_empty() {
            let _ = writeln!(self.out, "\nimpl {} {{", t.name);
            for f in &defaults {
                let lit = f
                    .default
                    .as_ref()
                    .map_or_else(String::new, |d| self.literal_json(&f.ty, d));
                let _ = writeln!(
                    self.out,
                    "    pub fn default_{}() -> {} {{\n        {lit}\n    }}",
                    f.name,
                    rust_type(&f.ty)
                );
            }
            self.out.push_str("}\n");
        }
        if fields.iter().all(|f| f.optional || f.default.is_some()) {
            let _ = writeln!(
                self.out,
                "\nimpl Default for {} {{\n    fn default() -> Self {{\n        Self {{",
                t.name
            );
            for f in fields {
                let v = if f.default.is_some() {
                    format!("Self::default_{}()", f.name)
                } else {
                    "None".into()
                };
                let _ = writeln!(self.out, "            {}: {v},", rust_ident(&f.name));
            }
            self.out.push_str("        }\n    }\n}\n");
        }
    }

    fn validate_impl(&mut self, t: &TypeDef) {
        let mut body = String::new();
        validate::type_body(self.schema, t, &mut body, &mut self.warnings);
        let unused = if body.trim().is_empty() {
            "        let _ = (path, errors);\n"
        } else {
            ""
        };
        let _ = writeln!(
            self.out,
            "\n#[allow(clippy::all, clippy::pedantic)]\nimpl {RT}::Validate for {} {{\n    fn validate_at(&self, path: &{RT}::Path<'_>, errors: &mut {RT}::ValidationErrors) {{\n{unused}{body}    }}\n}}",
            t.name
        );
    }

    fn literal_json(&self, ty: &Type, j: &korek_ir::Json) -> String {
        resolve(self.schema, ty, j)
            .map_or_else(|_| "unreachable!()".into(), |v| literal(self.schema, &v))
    }

    fn keys(&mut self, hooks: &[&dyn Hook]) {
        let s = self.schema;
        self.out.push_str("\n/// Every key, as a typed const named after its full dotted name.\n#[allow(clippy::all, clippy::pedantic)]\npub mod keys {\n    #[allow(unused_imports)]\n    use super::*;\n");
        let mut all = Vec::new();
        for g in &s.keys {
            let _ = writeln!(self.out, "\n    // `{}` keys.", g.name);
            comment(&mut self.out, "    ", "// ", doc_text(&g.attrs).as_deref());
            for k in &g.keys {
                self.key(g, k);
                all.push(format!("{:?}", format!("{}.{}", g.name, k.name)));
                for h in hooks {
                    h.after(Item::Key(g, k), &mut self.out);
                }
            }
            for h in hooks {
                h.after(Item::KeyGroup(g), &mut self.out);
            }
        }
        let _ = writeln!(
            self.out,
            "\n    /// Every key's full name.\n    pub const ALL: &[&str] = &[{}];\n}}",
            all.join(", ")
        );
    }

    fn key(&mut self, g: &KeyGroup, k: &Key) {
        let ty = rust_type(&k.ty);
        let ident = shouty(&g.key_ident(k));
        let (default, default_json) = match &k.default {
            Some(j) => (
                format!("Some(|| {})", self.literal_json(&k.ty, j)),
                format!("Some({:?})", j.to_string()),
            ),
            None => ("None".into(), "None".into()),
        };
        let mut check = String::new();
        validate::value_checks(
            self.schema,
            &k.ty,
            &k.validate,
            "v",
            &mut check,
            2,
            &mut self.warnings,
        );
        let check_fn = format!("check_{}", g.key_ident(k));
        let unused = if check.trim().is_empty() {
            "        let _ = (v, path, errors);\n"
        } else {
            ""
        };
        let _ = writeln!(
            self.out,
            "\n    fn {check_fn}(v: &{ty}, path: &{RT}::Path<'_>, errors: &mut {RT}::ValidationErrors) {{\n{unused}{check}    }}\n"
        );
        docs(&mut self.out, "    ", &k.attrs, &[]);
        let rules: Vec<String> = k
            .validate
            .rules
            .iter()
            .map(|r| {
                let operand = validate::operand_json(&r.kind);
                format!(
                    "{RT}::RuleInfo {{ name: {:?}, operand: {operand:?} }}",
                    r.kind.name()
                )
            })
            .collect();
        let opt = |o: &Option<String>| {
            o.as_ref()
                .map_or_else(|| "None".to_owned(), |s| format!("Some({s:?})"))
        };
        let _ = writeln!(
            self.out,
            "    pub const {ident}: {RT}::Key<{ty}> = {RT}::Key {{\n        group: {:?},\n        name: {:?},\n        path: {:?},\n        doc: {:?},\n        default: {default},\n        default_json: {default_json},\n        secret: {},\n        deprecated: {},\n        unit: {},\n        rules: &[{}],\n        check: {check_fn},\n    }};",
            g.name,
            k.name,
            format!("{}.{}", g.name, k.name),
            doc_text(&k.attrs).unwrap_or_default(),
            k.attrs.secret,
            opt(&k.attrs.deprecated),
            opt(&k.attrs.unit),
            rules.join(", "),
        );
    }
}

/// A Rust expression building `v`.
pub fn literal(schema: &Schema, v: &Value) -> String {
    let lit = |v: &Value| literal(schema, v);
    match v {
        Value::Bool(b) => b.to_string(),
        Value::String(s) | Value::Timestamp(s) | Value::Duration(s) => {
            format!("String::from({s:?})")
        }
        Value::Bytes(b) => format!(
            "vec!{:?}",
            korek_runtime::base64::decode(b).unwrap_or_default()
        ),
        Value::Int(i) => i.to_string(),
        Value::Uint(u) => u.to_string(),
        Value::Float(f) => format!("{f:?}"),
        Value::List(l) => format!("vec![{}]", l.iter().map(lit).collect::<Vec<_>>().join(", ")),
        Value::Map(m) if m.is_empty() => "::std::collections::BTreeMap::new()".into(),
        Value::Map(m) => {
            let items: Vec<String> = m
                .iter()
                .map(|(k, v)| format!("(String::from({k:?}), {})", lit(v)))
                .collect();
            format!("::std::collections::BTreeMap::from([{}])", items.join(", "))
        }
        Value::Enum { ty, variant } => format!("{ty}::{}", pascal(variant)),
        Value::Struct { ty, fields } => {
            let def = |name: &str| match schema.get(ty).map(|t| &t.kind) {
                Some(TypeKind::Struct(s)) => s.fields.iter().find(|f| f.name == name),
                _ => None,
            };
            let fs: Vec<String> = fields
                .iter()
                .map(|(k, v)| {
                    let f = def(k);
                    let has_default = f.is_some_and(|f| f.default.is_some());
                    let optional = f.is_some_and(|f| f.optional) && !has_default;
                    let v = match v {
                        None if has_default => format!("{ty}::default_{k}()"),
                        None => "None".into(),
                        Some(v) if optional => format!("Some({})", lit(v)),
                        Some(v) => lit(v),
                    };
                    format!("{}: {v}", rust_ident(k))
                })
                .collect();
            format!("{ty} {{ {} }}", fs.join(", "))
        }
        Value::Union { ty, variant, value } => format!("{ty}::{}({})", pascal(variant), lit(value)),
        _ => "unreachable!()".into(),
    }
}
