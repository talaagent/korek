//! Loading the example schema, and the errors for bad schemas.

extern crate alloc;

use std::path::Path;

use korek_core::ir::TypeKind;
use korek_core::{Failure, Loader, SourceFile};

fn example() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/basic/schema"
    ))
}

fn load(src: &str) -> Result<korek_core::Loaded, Failure> {
    Loader::new().load_sources(
        vec![SourceFile {
            path: "test.kdl".into(),
            contents: src.into(),
        }],
        None,
    )
}

#[track_caller]
fn fails(src: &str, code: &str) -> Failure {
    let f = load(src).expect_err("expected an error");
    assert!(f.has(code), "expected {code}, got:\n{f}");
    for d in &f.diagnostics {
        assert!(d.span().is_some(), "{d:?} has no span");
    }
    f
}

#[test]
fn loads_the_example() {
    let loaded = Loader::new().load(example()).unwrap();
    let s = &loaded.schema;
    assert_eq!(s.package, "example.v1");
    assert_eq!(s.files, ["keys.kdl", "service.kdl", "types.kdl"]);
    let TypeKind::Union(u) = &s.get("McpServer").unwrap().kind else {
        panic!()
    };
    assert_eq!(u.tag.as_deref(), Some("type"));
    let session = &s.keys[0];
    assert_eq!(session.keys[2].default, Some(serde_json::json!("600s")));
    assert_eq!(session.keys[3].default, Some(serde_json::json!(0.7)));
    let t = s.get("McpStdio").unwrap();
    assert_eq!(t.attrs.examples.len(), 1);
    let claude = &s.keys[1];
    assert!(claude.keys[2].attrs.secret);
    assert_eq!(claude.keys[2].attrs.meta[0].namespace, "acme.ui");
    assert_eq!(s.services[0].rpcs.len(), 2);
}

#[test]
fn numbers_are_stable_across_edits() {
    let first =
        load("struct A { field x \"string\"; field y \"string\"; field z \"string\" }").unwrap();
    let lock = SourceFile {
        path: "korek.lock".into(),
        contents: first.lock.render(),
    };
    // Remove y, add w: x and z keep 1 and 3, y's 2 is retired, w gets 4.
    let second = Loader::new()
        .load_sources(
            vec![SourceFile {
                path: "test.kdl".into(),
                contents: "struct A { field w \"string\"; field x \"string\"; field z \"string\" }"
                    .into(),
            }],
            Some(lock),
        )
        .unwrap();
    let TypeKind::Struct(st) = &second.schema.types[0].kind else {
        panic!()
    };
    let nums: Vec<_> = st
        .fields
        .iter()
        .map(|f| (f.name.as_str(), f.number))
        .collect();
    assert_eq!(nums, [("w", 4), ("x", 1), ("z", 3)]);
    assert_eq!(st.retired[0].name, "y");
    assert_eq!(st.retired[0].number, 2);
    let text = second.lock.render();
    assert!(text.contains("retired \"y\" 2"), "{text}");
}

#[test]
fn explicit_id_disagreeing_with_the_lock_fails() {
    let first = load("struct A { field x \"string\" }").unwrap();
    let f = Loader::new()
        .load_sources(
            vec![SourceFile {
                path: "test.kdl".into(),
                contents: "struct A { field x \"string\" id=7 }".into(),
            }],
            Some(SourceFile {
                path: "korek.lock".into(),
                contents: first.lock.render(),
            }),
        )
        .unwrap_err();
    assert!(f.has("korek::renumbered"), "{f}");
}

#[test]
fn explicit_ids_win() {
    let l = load("enum E { variant a id=5; variant b }").unwrap();
    let TypeKind::Enum(e) = &l.schema.types[0].kind else {
        panic!()
    };
    assert_eq!((e.variants[0].number, e.variants[1].number), (5, 6));
}

#[test]
fn unknown_type_suggests() {
    let f = fails(
        "struct A { field b \"Strng\" }\nstruct String2 {}",
        "korek::unknown_type",
    );
    assert!(f.diagnostics[0].message.contains("Strng"));
    let f = fails(
        "struct McpStdio {}\nunion U tag=\"t\" { variant a \"McpStdo\" }",
        "korek::unknown_type",
    );
    assert_eq!(
        f.diagnostics[0].help.as_deref(),
        Some("did you mean `McpStdio`?")
    );
}

#[test]
fn duplicate_names() {
    fails("struct A {}\nenum A { variant x }", "korek::duplicate_name");
    fails(
        "struct A { field x \"string\"; field x \"i64\" }",
        "korek::duplicate_name",
    );
    fails(
        "keys \"s\" { key \"a.b\" \"bool\"; key \"a_b\" \"bool\" }",
        "korek::duplicate_name",
    );
    fails(
        "struct SessionKeys {}\nkeys \"session\" {}",
        "korek::name_clash",
    );
}

#[test]
fn bad_defaults() {
    fails(
        "keys \"s\" { key \"a\" \"bool\" default=\"yes\" }",
        "korek::invalid_default",
    );
    fails(
        "keys \"s\" { key \"a\" \"u64\" default=-1 }",
        "korek::invalid_default",
    );
    fails(
        "enum E { variant x }\nkeys \"s\" { key \"a\" \"E\" default=\"y\" }",
        "korek::invalid_default",
    );
    fails(
        "keys \"s\" { key \"a\" \"list<i64>\" default=\"[1, \\\"x\\\"]\" }",
        "korek::invalid_default",
    );
    fails(
        "struct P { field n \"string\" }\nkeys \"s\" { key \"a\" \"P\" default=\"{}\" }",
        "korek::invalid_default",
    );
}

#[test]
fn json_values() {
    use korek_core::ir::Type;
    use serde_json::json;
    let loaded = load(
        r##"keys "s" {
  key "a" "json" default=#"{"x": [1, null]}"#
  key "b" "json" default=3
  key "c" "list<json>" default="[true, \"s\"]"
  key "d" "json" { validate { cel id="obj" expression="this != null" } }
}"##,
    )
    .unwrap();
    let keys = &loaded.schema.keys[0].keys;
    assert_eq!(keys[0].ty, Type::Json);
    assert_eq!(keys[0].default, Some(json!({"x": [1, null]})));
    assert_eq!(keys[1].default, Some(json!(3)));
    assert_eq!(keys[2].default, Some(json!([true, "s"])));
    // A string default is JSON text, as for lists and maps.
    fails(
        "keys \"s\" { key \"a\" \"json\" default=\"plain\" }",
        "korek::invalid_default",
    );
    // Only `cel` applies.
    fails(
        "struct A { field a \"json\" { validate { min_len 1 } } }",
        "korek::invalid_rule",
    );
    fails(
        "struct A { field a \"json\" { validate { const 1 } } }",
        "korek::invalid_rule",
    );
}

#[test]
fn constraint_violations() {
    fails(
        "keys \"s\" { key \"a\" \"u64\" default=0 { validate { gte 1 } } }",
        "korek::invalid_default",
    );
    fails(
        "keys \"s\" { key \"a\" \"string\" default=\"x\" { validate { min_len 2 } } }",
        "korek::invalid_default",
    );
    fails(
        "keys \"s\" { key \"a\" \"string\" default=\"x\" { validate { pattern \"^y\" } } }",
        "korek::invalid_default",
    );
    fails(
        "struct A { field a \"bool\" { validate { gte 1 } } }",
        "korek::invalid_rule",
    );
    fails(
        "struct A { field a \"i64\" { validate { gte 2; lte 1 } } }",
        "korek::invalid_rule",
    );
    fails(
        "struct A { field a \"string\" { validate { pattern \"(\" } } }",
        "korek::invalid_rule",
    );
    // Nested field constraints apply to defaults too.
    fails(
        "struct P { field n \"i64\" { validate { lte 3 } } }\nkeys \"s\" { key \"a\" \"P\" default=#\"{\"n\": 4}\"# }",
        "korek::invalid_default",
    );
}

#[test]
fn structural_errors() {
    fails("struct A { fild x \"string\" }", "korek::unknown_node");
    fails(
        "struct A { field x \"string\" { validate { emial } } }",
        "korek::unknown_rule",
    );
    fails(
        "struct A { field x \"string\" { example 5 } }",
        "korek::invalid_example",
    );
    fails(
        "struct A { field x \"u32\" default=5000000000 }",
        "korek::invalid_default",
    );
    fails(
        "struct A { field x \"list<string>\" { validate { keys { min_len 1 } } } }",
        "korek::invalid_rule",
    );
    fails(
        "struct A { field x \"string\" opt=#true }",
        "korek::unknown_property",
    );
    fails("widget A {}", "korek::unknown_node");
    fails(
        "union U tag=\"t\" { variant a \"string\" }",
        "korek::invalid_union",
    );
    fails(
        "struct A { field t \"string\" }\nunion U tag=\"t\" { variant a \"A\" }",
        "korek::invalid_union",
    );
    fails(
        "struct A { field b \"B\" }\nstruct B { field a \"list<A>\" }",
        "korek::cycle",
    );
    fails("struct a {}", "korek::invalid_name");
    fails("enum E { variant unspecified }", "korek::invalid_name");
    fails(
        "struct A { field x \"map<i64,string>\" }",
        "korek::invalid_type",
    );
    fails("struct A { field x \"string\" id=0 }", "korek::invalid_id");
    fails(
        "struct A { field x \"string\" id=2; field y \"string\" id=2 }",
        "korek::duplicate_id",
    );
    fails(
        "enum E { variant x }\nservice S { rpc Do input=\"E\" output=\"E\" }",
        "korek::invalid_rpc",
    );
    fails("struct A {", "korek::syntax");
}

#[test]
fn errors_render_with_spans() {
    let f = fails("struct A {\n  field b \"Nope\"\n}", "korek::unknown_type");
    let d = &f.diagnostics[0];
    let span = d.span().unwrap();
    let src = "struct A {\n  field b \"Nope\"\n}";
    assert_eq!(
        &src[span.offset as usize..(span.offset + span.len) as usize],
        "\"Nope\""
    );
    #[cfg(feature = "miette")]
    {
        let mut out = String::new();
        miette::GraphicalReportHandler::new_themed(miette::GraphicalTheme::unicode_nocolor())
            .render_report(&mut out, &f.rendered()[0])
            .unwrap();
        assert!(out.contains("test.kdl:2:11"), "{out}");
        assert!(out.contains("korek::unknown_type"), "{out}");
    }
}

#[test]
fn every_code_is_documented() {
    let docs =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/errors.md"))
            .unwrap();
    let documented: alloc::collections::BTreeSet<String> = docs
        .lines()
        .filter_map(|l| l.strip_prefix("## `").and_then(|l| l.strip_suffix('`')))
        .map(str::to_owned)
        .collect();
    let in_code: alloc::collections::BTreeSet<String> = korek_core::codes::ALL
        .iter()
        .map(|(c, _, _)| (*c).to_owned())
        .collect();
    assert_eq!(
        documented, in_code,
        "docs/errors.md and korek_ir::codes::ALL differ"
    );
}

#[test]
fn diagnostics_have_json_form() {
    let f = fails("struct A {\n  field b \"Nope\"\n}", "korek::unknown_type");
    let j = f.to_json();
    assert_eq!(j[0]["code"], "korek::unknown_type");
    assert_eq!(j[0]["labels"][0]["line"], 2);
    assert_eq!(j[0]["labels"][0]["col"], 11);
    assert_eq!(j[0]["labels"][0]["file"], "test.kdl");
}

/// `schema/korek` (korek's own types in korek) loads and matches the
/// hand-written protocol types' JSON field names.
#[test]
fn self_hosting_schema_loads() {
    let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schema/korek"));
    let loaded = Loader::new()
        .lock_path(std::env::temp_dir().join("korek-selfhost.lock"))
        .load(dir)
        .unwrap();
    let resp = korek_core::ir::GenerateResponse::new(vec![korek_core::ir::File::new("a", "b")]);
    let json = serde_json::to_value(&resp).unwrap();
    let TypeKind::Struct(s) = &loaded.schema.get("GenerateResponse").unwrap().kind else {
        panic!()
    };
    let names: Vec<&str> = s.fields.iter().map(|f| f.name.as_str()).collect();
    let keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(names, keys);
}
