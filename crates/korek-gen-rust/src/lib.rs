//! Rust: serde types, a `korek_runtime::Validate` impl per type, `Default`
//! impls where every field has a default, and a `keys` module of typed key
//! consts (`pub const SESSION_STEER_ENABLED: Key<bool>`).
//!
//! The output is one file, `types.rs` by default (option `file`), with no
//! `use` items and no inner attributes, so it can be `include!`d. It needs
//! `serde` (with `derive`) and `korek-runtime` in the including crate.

#![no_std]

extern crate alloc;

use alloc::{borrow::ToOwned, format, string::String, string::ToString, vec, vec::Vec};

mod validate;

use core::fmt::Write as _;

use korek_gen_common::{GENERATED, comment, doc_text, rust_ident};
use korek_ir::naming::{pascal, shouty};
use korek_ir::value::{Value, resolve};
use korek_ir::{
    Diagnostic, Field, Key, KeyGroup, Schema, Type, TypeDef, TypeIndex, TypeKind, Types,
};
use korek_plugin::{File, GenerateRequest, GenerateResponse, Hook, Item};

/// Generator options.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Options {
    pub file: String,
    /// Integer fields and keys with a time `unit` (`nanoseconds` through
    /// `hours`) become `core::time::Duration`, still the integer in JSON.
    pub duration: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            file: "types.rs".into(),
            duration: false,
        }
    }
}

/// The plugin entry point (`korek-gen-rust`).
///
/// # Errors
/// On unknown options.
pub fn plugin(req: &GenerateRequest) -> Result<GenerateResponse, Diagnostic> {
    req.check_options(&["file", "rust_duration"])?;
    let mut opts = Options::default();
    if let Some(f) = req.options.get("file") {
        opts.file.clone_from(f);
    }
    if let Some(v) = req.options.get("rust_duration") {
        opts.duration = v.parse().map_err(|_| {
            Diagnostic::error(
                korek_ir::codes::INVALID_OPTION,
                format!("rust_duration must be true or false, not `{v}`"),
            )
        })?;
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
        Type::Json => format!("{RT}::serde_json::Value"),
        Type::Named(n) => n.clone(),
    }
}

/// The runtime codec for a type holding `bytes` below the top level
/// (`list<bytes>`, `map<string,list<bytes>>`, …).
fn nested_codec(ty: &Type) -> Option<String> {
    match ty {
        Type::Bytes => Some(format!("{RT}::base64::Base64")),
        Type::List(t) => nested_codec(t).map(|c| format!("{RT}::with::List<{c}>")),
        Type::Map(t) => nested_codec(t).map(|c| format!("{RT}::with::Map<{c}>")),
        _ => None,
    }
}

/// The `#[serde(with)]` target for a value of `ty` (`opt`: in an `Option`),
/// or `None` when serde's own form is the JSON form.
fn serde_with(ty: &Type, unit: Option<&str>, opt: bool) -> Option<String> {
    let codec = match unit {
        Some(u) => format!("{RT}::time::{u}"),
        None if *ty == Type::Bytes => {
            return Some(format!("{RT}::base64{}", if opt { "::option" } else { "" }));
        }
        None => nested_codec(ty)?,
    };
    let codec = if opt {
        format!("{RT}::with::Opt<{codec}>")
    } else {
        codec
    };
    Some(format!("{RT}::with::As::<{codec}>"))
}

/// With the `rust_duration` option: the runtime unit for an integer with a
/// time `unit`.
pub(crate) fn duration_unit(on: bool, ty: &Type, attrs: &korek_ir::Attrs) -> Option<&'static str> {
    if !on || !ty.is_integer() {
        return None;
    }
    Some(match attrs.unit.as_deref()? {
        "nanoseconds" => "Nanoseconds",
        "microseconds" => "Microseconds",
        "milliseconds" => "Milliseconds",
        "seconds" => "Seconds",
        "minutes" => "Minutes",
        "hours" => "Hours",
        _ => return None,
    })
}

const DURATION: &str = "::core::time::Duration";

/// A `Duration` expression for `n` of `unit`.
fn duration_literal(unit: &str, v: &Value) -> String {
    let n = match v {
        Value::Int(i) => u64::try_from(*i).unwrap_or_default(),
        Value::Uint(u) => *u,
        _ => 0,
    };
    match unit {
        "Nanoseconds" => format!("{DURATION}::from_nanos({n})"),
        "Microseconds" => format!("{DURATION}::from_micros({n})"),
        "Milliseconds" => format!("{DURATION}::from_millis({n})"),
        "Minutes" => format!("{DURATION}::from_secs({})", n.saturating_mul(60)),
        "Hours" => format!("{DURATION}::from_secs({})", n.saturating_mul(3600)),
        _ => format!("{DURATION}::from_secs({n})"),
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
    opts: &'a Options,
    index: TypeIndex<'a>,
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
        opts,
        index: schema.index(),
        out: String::with_capacity(4096 + schema.types.len() * 2048),
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
                    if let Some(w) = serde_with(&v.ty, None, false) {
                        let _ = writeln!(self.out, "    #[serde(with = \"{w}\")]");
                    }
                    let _ = writeln!(self.out, "    {}({}),", pascal(&v.name), rust_type(&v.ty));
                }
                self.out.push_str("}\n");
            }
        }
        self.validate_impl(t);
    }

    fn unit(&self, ty: &Type, attrs: &korek_ir::Attrs) -> Option<&'static str> {
        duration_unit(self.opts.duration, ty, attrs)
    }

    /// The Rust type of a field or key, before `Option`.
    fn value_type(&self, ty: &Type, attrs: &korek_ir::Attrs) -> String {
        if self.unit(ty, attrs).is_some() {
            DURATION.into()
        } else {
            rust_type(ty)
        }
    }

    /// The field's Rust type: `Option<T>` when optional without a default.
    fn field_type(&self, f: &Field) -> String {
        let ty = self.value_type(&f.ty, &f.attrs);
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
            if let Some(w) = serde_with(&f.ty, self.unit(&f.ty, &f.attrs), opt) {
                serde.push(format!("with = \"{w}\""));
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
                self.field_type(f)
            );
        }
        self.out.push_str("}\n");
        let defaults: Vec<&Field> = fields.iter().filter(|f| f.default.is_some()).collect();
        if !defaults.is_empty() {
            let _ = writeln!(self.out, "\nimpl {} {{", t.name);
            for f in &defaults {
                let lit = f.default.as_ref().map_or_else(String::new, |d| {
                    self.literal_json(&f.ty, self.unit(&f.ty, &f.attrs), d)
                });
                let _ = writeln!(
                    self.out,
                    "    pub fn default_{}() -> {} {{\n        {lit}\n    }}",
                    f.name,
                    self.value_type(&f.ty, &f.attrs)
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
        validate::type_body(
            &self.index,
            t,
            self.opts.duration,
            &mut body,
            &mut self.warnings,
        );
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

    fn literal_json(&self, ty: &Type, unit: Option<&str>, j: &korek_ir::Json) -> String {
        resolve(&self.index, ty, j).map_or_else(
            |_| "unreachable!()".into(),
            |v| match unit {
                Some(u) => duration_literal(u, &v),
                None => literal_with(&self.index, &v, self.opts.duration),
            },
        )
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
        let unit = self.unit(&k.ty, &k.attrs);
        let ty = self.value_type(&k.ty, &k.attrs);
        let ident = shouty(&g.key_ident(k));
        let (default, default_json) = match &k.default {
            Some(j) => (
                format!("Some(|| {})", self.literal_json(&k.ty, unit, j)),
                format!("Some({:?})", j.to_string()),
            ),
            None => ("None".into(), "None".into()),
        };
        let mut check = String::new();
        validate::value_checks(
            &self.index,
            &k.ty,
            &k.validate,
            "v",
            &mut check,
            2,
            &mut self.warnings,
        );
        if let (Some(u), false) = (unit, check.is_empty()) {
            check.insert_str(0, &validate::duration_as_int(u, 2));
        }
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
pub fn literal<S: Types + ?Sized>(schema: &S, v: &Value) -> String {
    literal_with(schema, v, false)
}

/// [`literal`], with `duration` the `rust_duration` option.
fn literal_with<S: Types + ?Sized>(schema: &S, v: &Value, duration: bool) -> String {
    let lit = |v: &Value| literal_with(schema, v, duration);
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
                    let unit = f.and_then(|f| duration_unit(duration, &f.ty, &f.attrs));
                    let lit = |v: &Value| unit.map_or_else(|| lit(v), |u| duration_literal(u, v));
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
        Value::Json(j) => format!(
            "{RT}::serde_json::from_str::<{RT}::serde_json::Value>({:?}).expect(\"valid JSON\")",
            j.to_string()
        ),
        _ => "unreachable!()".into(),
    }
}
