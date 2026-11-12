//! `schema/korek` describes korek-ir's types: real IR values serialized by
//! korek-ir deserialize through the Rust generated from `schema/korek` and
//! serialize back to the same JSON (so no field is missing, renamed, or of
//! another shape), and pass the generated `validate()`.

#![allow(clippy::unwrap_used)]

use std::path::Path;

use korek_fixture::selfhost as kg;
use korek_ir::{
    Diagnostic, Format, GenerateRequest, GenerateResponse, Label, Level, Meta, Retired, Rule,
    RuleKind, Schema, Span, TypeKind, Ui,
};
use korek_runtime::Validate as _;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

fn dir(rel: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

/// Serializes `v` with korek-ir, reads it back as `G` and serializes that.
#[track_caller]
fn same_json<G: DeserializeOwned + Serialize>(v: &impl Serialize) -> G {
    let j = serde_json::to_value(v).unwrap();
    let g: G = serde_json::from_value(j.clone())
        .unwrap_or_else(|e| panic!("the generated type rejects korek-ir's JSON: {e}"));
    assert_eq!(serde_json::to_value(&g).unwrap(), j);
    g
}

#[track_caller]
fn check(schema: Schema) {
    let req = GenerateRequest::new("rust", schema);
    let g: kg::GenerateRequest = same_json(&req);
    assert_eq!(g.validate(), Ok(()));
    // And back: the generated JSON is korek-ir's.
    let back: GenerateRequest = serde_json::from_value(serde_json::to_value(&g).unwrap()).unwrap();
    assert_eq!(back, req);
}

/// Sets the fields the samples leave empty.
fn fill(mut s: Schema) -> Schema {
    s.ext
        .insert("acme".into(), vec![json!({"a": 1}), json!(null)]);
    let t = &mut s.types[0];
    t.attrs.ui = Some(Ui {
        label: Some("L".into()),
        section: Some("S".into()),
        level: Some(Level::Advanced),
        widget: Some("select".into()),
    });
    t.attrs.meta.push(Meta {
        namespace: "acme.ui".into(),
        props: [("k".to_owned(), json!([1, "x"]))].into(),
    });
    t.attrs.ext.insert("acme".into(), vec![json!("x")]);
    t.attrs.description = Some("Long.".into());
    t.attrs.unit = Some("seconds".into());
    let retired = Retired {
        name: "old".into(),
        number: 9,
    };
    for t in &mut s.types {
        match &mut t.kind {
            TypeKind::Enum(e) => e.retired.push(retired.clone()),
            TypeKind::Struct(st) => st.retired.push(retired.clone()),
            TypeKind::Union(u) => u.retired.push(retired.clone()),
        }
    }
    for g in &mut s.keys {
        g.retired.push(retired.clone());
    }
    s
}

#[test]
fn the_example_schema_round_trips() {
    let loaded = korek_core::Loader::new()
        .load(&dir("examples/basic/schema"))
        .unwrap();
    check(loaded.schema.clone());
    check(fill(loaded.schema));
}

#[test]
fn every_rule_round_trips() {
    let loaded = korek_core::Loader::new()
        .load_sources(
            vec![korek_core::SourceFile {
                path: "vectors.kdl".into(),
                contents: korek_testkit::schema(),
            }],
            None,
        )
        .unwrap();
    check(loaded.schema);
}

/// One of every rule kind (add new kinds here and to `schema/korek/ir.kdl`).
fn every_rule() -> Vec<RuleKind> {
    use RuleKind as R;
    let v = || json!(1);
    let s = || "x".to_owned();
    vec![
        R::Const { value: v() },
        R::In { values: vec![v()] },
        R::NotIn { values: vec![v()] },
        R::Len { n: 1 },
        R::MinLen { n: 1 },
        R::MaxLen { n: 1 },
        R::LenBytes { n: 1 },
        R::MinBytes { n: 1 },
        R::MaxBytes { n: 1 },
        R::Pattern { regex: s() },
        R::Prefix { value: s() },
        R::Suffix { value: s() },
        R::Contains { value: s() },
        R::NotContains { value: s() },
        R::Format {
            format: Format::Email,
        },
        R::Gt { value: v() },
        R::Gte { value: v() },
        R::Lt { value: v() },
        R::Lte { value: v() },
        R::Finite,
        R::MultipleOf { value: v() },
        R::MinItems { n: 1 },
        R::MaxItems { n: 1 },
        R::Unique,
        R::MinPairs { n: 1 },
        R::MaxPairs { n: 1 },
        R::Required,
        R::DefinedOnly,
        R::GtNow,
        R::LtNow,
        R::Within { duration: s() },
        R::Cel {
            id: s(),
            expression: s(),
            ts: Some(s()),
            rust: Some(s()),
        },
    ]
}

#[test]
fn every_rule_kind_round_trips() {
    let rules: Vec<Rule> = every_rule()
        .into_iter()
        .map(|kind| Rule {
            kind,
            message: Some("m".into()),
            span: Span::default(),
        })
        .collect();
    let g: Vec<kg::Rule> = same_json(&rules);
    assert_eq!(g.len(), rules.len());
    // The schema has no rule kind korek-ir lacks.
    let loaded = korek_core::Loader::new()
        .load(&dir("schema/korek"))
        .unwrap();
    let TypeKind::Union(u) = &loaded.schema.get("RuleKind").unwrap().kind else {
        panic!()
    };
    let tags: Vec<String> = rules
        .iter()
        .map(|r| {
            serde_json::to_value(&r.kind).unwrap()["rule"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    let names: Vec<&str> = u.variants.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, tags);
    // Every format in the schema is one of korek-ir's.
    let TypeKind::Enum(e) = &loaded.schema.get("Format").unwrap().kind else {
        panic!()
    };
    for v in &e.variants {
        let f: Format = serde_json::from_value(json!(v.name)).unwrap();
        assert_eq!(f.name().trim_start_matches("iso_"), v.name);
    }
}

#[test]
fn every_type_round_trips() {
    use korek_ir::Type as T;
    let mut types: Vec<T> = [
        "bool", "string", "bytes", "i32", "i64", "u32", "u64", "f32", "f64",
    ]
    .into_iter()
    .chain(["timestamp", "duration", "json"])
    .map(|n| T::primitive(n).unwrap())
    .collect();
    types.push(T::List(Box::new(T::Map(Box::new(T::Named("A".into()))))));
    let g: Vec<kg::Type> = same_json(&types);
    assert!(matches!(g[0], kg::Type::Primitive(kg::Primitive::Bool)));
    assert!(matches!(g[12], kg::Type::List(_)));
}

#[test]
fn korek_schema_round_trips() {
    let loaded = korek_core::Loader::new()
        .load(&dir("schema/korek"))
        .unwrap();
    check(loaded.schema);
}

#[test]
fn responses_round_trip() {
    let mut warning = Diagnostic::warning("korek::rule_unsupported", "no");
    warning.help = Some("drop it".into());
    let mut error = Diagnostic::error("korek_gen_x::bad", "bad");
    error.labels.push(Label {
        span: Span {
            file: 1,
            offset: 2,
            len: 3,
        },
        text: Some("here".into()),
    });
    let resp = GenerateResponse::new(vec![korek_ir::File::new("a.rs", "fn main() {}")])
        .with_diagnostics(vec![warning, error]);
    let g: kg::GenerateResponse = same_json(&resp);
    assert_eq!(g.validate(), Ok(()));
}
