//! JSON Schema 2020-12: one `schema.json` with a `$defs` entry per type and
//! a `Keys` object (keys by full dotted name), or with `per_type=true` one
//! self-contained `<Name>.schema.json` per type (e.g. MCP tool inputs).
//! Docs become `description`, examples `examples`, defaults `default`, and
//! as many rules as JSON Schema can express become keywords.
//!
//! Options: `file` (default `schema.json`), `per_type` (default `false`).

extern crate alloc;

use alloc::collections::BTreeSet;

use korek_gen_common::{doc_text, format_regex, unsupported};
use korek_ir::{
    Attrs, Diagnostic, Format, Json, RuleKind, Schema, Type, TypeDef, TypeKind, Validate,
};
use korek_plugin::{File, GenerateRequest, GenerateResponse, Hook};
use serde_json::{Map, json};

/// Generator options.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Options {
    pub file: String,
    pub per_type: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            file: "schema.json".into(),
            per_type: false,
        }
    }
}

/// The plugin entry point (`korek-gen-jsonschema`).
///
/// # Errors
/// On unknown or malformed options.
pub fn plugin(req: &GenerateRequest) -> Result<GenerateResponse, Diagnostic> {
    req.check_options(&["file", "per_type"])?;
    let mut opts = Options {
        per_type: req.bool_option("per_type", false)?,
        ..Options::default()
    };
    if let Some(f) = req.options.get("file") {
        opts.file.clone_from(f);
    }
    let (files, warnings) = generate(&req.schema, &opts, &[]);
    Ok(GenerateResponse::new(files).with_diagnostics(warnings))
}

const DRAFT: &str = "https://json-schema.org/draft/2020-12/schema";

struct Gen<'a> {
    schema: &'a Schema,
    warnings: Vec<Diagnostic>,
}

/// Adds `key: value`, moving it into `allOf` when the key is taken.
fn put(o: &mut Map<String, Json>, key: &str, value: Json) {
    if o.contains_key(key) {
        let all = o.entry("allOf").or_insert_with(|| json!([]));
        if let Some(a) = all.as_array_mut() {
            a.push(json!({ key: value }));
        }
    } else {
        o.insert(key.into(), value);
    }
}

fn escape(s: &str) -> String {
    s.chars().fold(String::new(), |mut o, c| {
        if "\\^$.|?*+()[]{}/".contains(c) {
            o.push('\\');
        }
        o.push(c);
        o
    })
}

fn annotate(o: &mut Map<String, Json>, attrs: &Attrs) {
    if let Some(d) = doc_text(attrs) {
        o.insert("description".into(), d.into());
    }
    if let Some(l) = attrs.ui.as_ref().and_then(|u| u.label.clone()) {
        o.insert("title".into(), l.into());
    }
    if !attrs.examples.is_empty() && !attrs.secret {
        o.insert("examples".into(), Json::Array(attrs.examples.clone()));
    }
    if attrs.deprecated.is_some() {
        o.insert("deprecated".into(), true.into());
    }
    if attrs.secret {
        o.insert("writeOnly".into(), true.into());
    }
}

fn base(ty: &Type) -> Map<String, Json> {
    let v = match ty {
        Type::Bool => json!({"type": "boolean"}),
        Type::String => json!({"type": "string"}),
        Type::Bytes => json!({"type": "string", "contentEncoding": "base64"}),
        Type::I32 => json!({"type": "integer", "minimum": i32::MIN, "maximum": i32::MAX}),
        Type::U32 => json!({"type": "integer", "minimum": 0, "maximum": u32::MAX}),
        Type::I64 => json!({"type": "integer"}),
        Type::U64 => json!({"type": "integer", "minimum": 0}),
        Type::F32 | Type::F64 => json!({"type": "number"}),
        Type::Timestamp => json!({"type": "string", "format": "date-time"}),
        Type::Duration => json!({"type": "string", "pattern": "^-?\\d+(\\.\\d+)?s$"}),
        Type::Json => json!({}),
        Type::List(_) => json!({"type": "array"}),
        Type::Map(_) => json!({"type": "object"}),
        Type::Named(n) => json!({"$ref": format!("#/$defs/{n}")}),
    };
    v.as_object().cloned().unwrap_or_default()
}

impl Gen<'_> {
    #[allow(clippy::too_many_lines)]
    fn value(&mut self, ty: &Type, v: &Validate) -> Map<String, Json> {
        let mut o = base(ty);
        let empty = Validate::default();
        match ty {
            Type::List(t) => {
                o.insert(
                    "items".into(),
                    Json::Object(self.value(t, v.items.as_deref().unwrap_or(&empty))),
                );
            }
            Type::Map(t) => {
                o.insert(
                    "additionalProperties".into(),
                    Json::Object(self.value(t, v.values.as_deref().unwrap_or(&empty))),
                );
                if let Some(k) = &v.keys {
                    o.insert(
                        "propertyNames".into(),
                        Json::Object(self.value(&Type::String, k)),
                    );
                }
            }
            _ => {}
        }
        for r in &v.rules {
            let is_str = *ty == Type::String;
            match &r.kind {
                RuleKind::Const { value } => put(&mut o, "const", value.clone()),
                RuleKind::In { values } => put(&mut o, "enum", Json::Array(values.clone())),
                RuleKind::NotIn { values } => put(&mut o, "not", json!({"enum": values})),
                RuleKind::Len { n } if is_str => {
                    put(&mut o, "minLength", (*n).into());
                    put(&mut o, "maxLength", (*n).into());
                }
                RuleKind::MinLen { n } if is_str => put(&mut o, "minLength", (*n).into()),
                RuleKind::MaxLen { n } if is_str => put(&mut o, "maxLength", (*n).into()),
                RuleKind::Pattern { regex } if is_str => {
                    put(&mut o, "pattern", regex.as_str().into());
                }
                RuleKind::Prefix { value } if is_str => {
                    put(&mut o, "pattern", format!("^{}", escape(value)).into());
                }
                RuleKind::Suffix { value } if is_str => {
                    put(&mut o, "pattern", format!("{}$", escape(value)).into());
                }
                RuleKind::Contains { value } if is_str => {
                    put(&mut o, "pattern", escape(value).into());
                }
                RuleKind::NotContains { value } => {
                    put(&mut o, "not", json!({"pattern": escape(value)}));
                }
                RuleKind::Format { format } if is_str => {
                    let native = match format {
                        Format::Email => Some("email"),
                        Format::Hostname => Some("hostname"),
                        Format::Ipv4 => Some("ipv4"),
                        Format::Ipv6 => Some("ipv6"),
                        Format::Uri | Format::Url => Some("uri"),
                        Format::UriRef => Some("uri-reference"),
                        Format::Uuid => Some("uuid"),
                        Format::Datetime => Some("date-time"),
                        Format::Date => Some("date"),
                        Format::Time => Some("time"),
                        Format::Duration => Some("duration"),
                        _ => None,
                    };
                    match (native, format_regex(*format), format) {
                        (Some(f), _, _) => put(&mut o, "format", f.into()),
                        (None, Some(re), _) => put(&mut o, "pattern", re.into()),
                        (None, None, Format::Ip) => put(
                            &mut o,
                            "anyOf",
                            json!([{"format": "ipv4"}, {"format": "ipv6"}]),
                        ),
                        _ => self.warnings.push(unsupported(
                            "jsonschema",
                            r,
                            "no JSON Schema keyword for this format",
                        )),
                    }
                }
                RuleKind::Gt { value } if ty.is_numeric() => {
                    put(&mut o, "exclusiveMinimum", value.clone());
                }
                RuleKind::Gte { value } if ty.is_numeric() => put(&mut o, "minimum", value.clone()),
                RuleKind::Lt { value } if ty.is_numeric() => {
                    put(&mut o, "exclusiveMaximum", value.clone());
                }
                RuleKind::Lte { value } if ty.is_numeric() => put(&mut o, "maximum", value.clone()),
                RuleKind::MultipleOf { value } => put(&mut o, "multipleOf", value.clone()),
                RuleKind::MinItems { n } => put(&mut o, "minItems", (*n).into()),
                RuleKind::MaxItems { n } => put(&mut o, "maxItems", (*n).into()),
                RuleKind::Unique => put(&mut o, "uniqueItems", true.into()),
                RuleKind::MinPairs { n } => put(&mut o, "minProperties", (*n).into()),
                RuleKind::MaxPairs { n } => put(&mut o, "maxProperties", (*n).into()),
                RuleKind::Finite | RuleKind::Required | RuleKind::DefinedOnly => {}
                _ => self.warnings.push(unsupported(
                    "jsonschema",
                    r,
                    "no JSON Schema keyword for this rule",
                )),
            }
        }
        o
    }

    fn def(&mut self, t: &TypeDef) -> Json {
        let mut o = Map::new();
        match &t.kind {
            TypeKind::Enum(e) => {
                o.insert("type".into(), "string".into());
                o.insert(
                    "enum".into(),
                    e.variants
                        .iter()
                        .map(|v| Json::from(v.name.as_str()))
                        .collect(),
                );
            }
            TypeKind::Struct(s) => {
                o.insert("type".into(), "object".into());
                let mut props = Map::new();
                let mut required = Vec::new();
                for f in &s.fields {
                    let mut p = self.value(&f.ty, &f.validate);
                    annotate(&mut p, &f.attrs);
                    if let Some(d) = &f.default {
                        p.insert("default".into(), d.clone());
                    }
                    if !f.optional && f.default.is_none() {
                        required.push(Json::from(f.name.as_str()));
                    }
                    props.insert(f.name.clone(), Json::Object(p));
                }
                o.insert("properties".into(), Json::Object(props));
                if !required.is_empty() {
                    o.insert("required".into(), Json::Array(required));
                }
            }
            TypeKind::Union(u) => {
                let variants: Vec<Json> = u
                    .variants
                    .iter()
                    .map(|v| {
                        let mut s = self.value(&v.ty, &Validate::default());
                        if let Some(tag) = &u.tag {
                            s.insert("properties".into(), json!({ tag: {"const": v.name} }));
                            s.insert("required".into(), json!([tag]));
                        }
                        annotate(&mut s, &v.attrs);
                        Json::Object(s)
                    })
                    .collect();
                o.insert(
                    if u.tag.is_some() { "oneOf" } else { "anyOf" }.into(),
                    Json::Array(variants),
                );
            }
        }
        annotate(&mut o, &t.attrs);
        Json::Object(o)
    }

    fn keys(&mut self) -> Json {
        let mut props = Map::new();
        for g in &self.schema.keys {
            for k in &g.keys {
                let mut p = self.value(&k.ty, &k.validate);
                annotate(&mut p, &k.attrs);
                if let Some(d) = k.default.as_ref().filter(|_| !k.attrs.secret) {
                    p.insert("default".into(), d.clone());
                }
                props.insert(format!("{}.{}", g.name, k.name), Json::Object(p));
            }
        }
        json!({"type": "object", "description": "Every key, by its full dotted name.", "properties": props})
    }
}

/// Names a type refers to, transitively, including itself.
fn reachable<'a>(schema: &'a Schema, name: &'a str, out: &mut BTreeSet<&'a str>) {
    fn named(ty: &Type) -> Option<&str> {
        match ty {
            Type::Named(n) => Some(n),
            Type::List(t) | Type::Map(t) => named(t),
            _ => None,
        }
    }
    if !out.insert(name) {
        return;
    }
    let Some(t) = schema.get(name) else { return };
    let tys: Vec<&Type> = match &t.kind {
        TypeKind::Struct(s) => s.fields.iter().map(|f| &f.ty).collect(),
        TypeKind::Union(u) => u.variants.iter().map(|v| &v.ty).collect(),
        TypeKind::Enum(_) => Vec::new(),
    };
    for ty in tys {
        if let Some(n) = named(ty) {
            reachable(schema, n, out);
        }
    }
}

fn pretty(v: &Json) -> String {
    let mut s = serde_json::to_string_pretty(v).unwrap_or_default();
    s.push('\n');
    s
}

/// Generates the JSON Schema file(s) and warnings.
pub fn generate(
    schema: &Schema,
    opts: &Options,
    hooks: &[&dyn Hook],
) -> (Vec<File>, Vec<Diagnostic>) {
    let mut g = Gen {
        schema,
        warnings: Vec::new(),
    };
    let defs: Map<String, Json> = schema
        .types
        .iter()
        .map(|t| (t.name.clone(), g.def(t)))
        .collect();
    let mut files = Vec::new();
    if opts.per_type {
        for t in &schema.types {
            let mut names = BTreeSet::new();
            reachable(schema, &t.name, &mut names);
            names.remove(t.name.as_str());
            let mut root = defs
                .get(&t.name)
                .and_then(Json::as_object)
                .cloned()
                .unwrap_or_default();
            root.insert("$schema".into(), DRAFT.into());
            if !names.is_empty() {
                let sub: Map<String, Json> = names
                    .iter()
                    .filter_map(|n| defs.get(*n).map(|d| ((*n).to_owned(), d.clone())))
                    .collect();
                root.insert("$defs".into(), Json::Object(sub));
            }
            files.push(File::new(
                format!("{}.schema.json", t.name),
                pretty(&Json::Object(root)),
            ));
        }
    } else {
        let mut root = Map::new();
        root.insert("$schema".into(), DRAFT.into());
        root.insert("$comment".into(), korek_gen_common::GENERATED.into());
        let mut all = defs;
        if !schema.keys.is_empty() {
            all.insert("Keys".into(), g.keys());
        }
        root.insert("$defs".into(), Json::Object(all));
        files.push(File::new(&opts.file, pretty(&Json::Object(root))));
    }
    for h in hooks {
        files.extend(h.files(schema));
    }
    (files, g.warnings)
}
