//! Loading the example schema, and the errors for bad schemas.

use std::path::Path;

use korek_core::ir::TypeKind;
use korek_core::{Failure, Loader, SourceFile};

fn example() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/basic/schema"))
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
        assert!(d.span.is_some(), "{d:?} has no span");
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
    assert_eq!(session.keys[2].default, Some(serde_json::json!(600)));
    assert_eq!(session.keys[3].default, Some(serde_json::json!(0.7)));
    assert_eq!(s.services[0].rpcs.len(), 2);
}

#[test]
fn numbers_are_stable_across_edits() {
    let first = load("struct A { field x \"string\"; field y \"string\"; field z \"string\" }").unwrap();
    let lock = SourceFile {
        path: "korek.lock".into(),
        contents: first.lock.render(),
    };
    // Remove y, add w: x and z keep 1 and 3, y's 2 is retired, w gets 4.
    let second = Loader::new()
        .load_sources(
            vec![SourceFile {
                path: "test.kdl".into(),
                contents: "struct A { field w \"string\"; field x \"string\"; field z \"string\" }".into(),
            }],
            Some(lock),
        )
        .unwrap();
    let TypeKind::Struct(st) = &second.schema.types[0].kind else {
        panic!()
    };
    let nums: Vec<_> = st.fields.iter().map(|f| (f.name.as_str(), f.number)).collect();
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
    let f = fails("struct A { field b \"Strng\" }\nstruct String2 {}", "korek::unknown_type");
    assert!(f.diagnostics[0].message.contains("Strng"));
    let f = fails("struct McpStdio {}\nunion U tag=\"t\" { variant a \"McpStdo\" }", "korek::unknown_type");
    assert_eq!(f.diagnostics[0].help.as_deref(), Some("did you mean `McpStdio`?"));
}

#[test]
fn duplicate_names() {
    fails("struct A {}\nenum A { variant x }", "korek::duplicate");
    fails("struct A { field x \"string\"; field x \"i64\" }", "korek::duplicate");
    fails("keys \"s\" { key \"a.b\" \"bool\"; key \"a_b\" \"bool\" }", "korek::duplicate");
    fails("struct SessionKeys {}\nkeys \"session\" {}", "korek::name_clash");
}

#[test]
fn bad_defaults() {
    fails("keys \"s\" { key \"a\" \"bool\" default=\"yes\" }", "korek::bad_default");
    fails("keys \"s\" { key \"a\" \"u64\" default=-1 }", "korek::bad_default");
    fails("enum E { variant x }\nkeys \"s\" { key \"a\" \"E\" default=\"y\" }", "korek::bad_default");
    fails("keys \"s\" { key \"a\" \"list<i64>\" default=\"[1, \\\"x\\\"]\" }", "korek::bad_default");
    fails(
        "struct P { field n \"string\" }\nkeys \"s\" { key \"a\" \"P\" default=\"{}\" }",
        "korek::bad_default",
    );
}

#[test]
fn constraint_violations() {
    fails("keys \"s\" { key \"a\" \"u64\" default=0 { min 1 } }", "korek::bad_default");
    fails("keys \"s\" { key \"a\" \"string\" default=\"x\" { min_len 2 } }", "korek::bad_default");
    fails("keys \"s\" { key \"a\" \"string\" default=\"x\" { pattern \"^y\" } }", "korek::bad_default");
    fails("struct A { field a \"bool\" { min 1 } }", "korek::bad_constraint");
    fails("struct A { field a \"i64\" { min 2; max 1 } }", "korek::bad_constraint");
    fails("struct A { field a \"string\" { pattern \"(\" } }", "korek::bad_constraint");
    // Nested field constraints apply to defaults too.
    fails(
        "struct P { field n \"i64\" { max 3 } }\nkeys \"s\" { key \"a\" \"P\" default=#\"{\"n\": 4}\"# }",
        "korek::bad_default",
    );
}

#[test]
fn structural_errors() {
    fails("struct A { fild x \"string\" }", "korek::unknown_node");
    fails("struct A { field x \"string\" opt=#true }", "korek::unknown_property");
    fails("widget A {}", "korek::unknown_node");
    fails("union U tag=\"t\" { variant a \"string\" }", "korek::bad_union");
    fails("struct A { field t \"string\" }\nunion U tag=\"t\" { variant a \"A\" }", "korek::tag_clash");
    fails("struct A { field b \"B\" }\nstruct B { field a \"list<A>\" }", "korek::recursive_type");
    fails("struct a {}", "korek::bad_name");
    fails("enum E { variant unspecified }", "korek::bad_name");
    fails("struct A { field x \"map<i64,string>\" }", "korek::bad_type");
    fails("struct A { field x \"string\" id=0 }", "korek::bad_id");
    fails("struct A { field x \"string\" id=2; field y \"string\" id=2 }", "korek::duplicate_id");
    fails("enum E { variant x }\nservice S { rpc Do input=\"E\" output=\"E\" }", "korek::bad_rpc");
    fails("struct A {", "korek::syntax");
}

#[test]
fn errors_render_with_spans() {
    let f = fails("struct A {\n  field b \"Nope\"\n}", "korek::unknown_type");
    let d = &f.diagnostics[0];
    let span = d.span.unwrap();
    let src = "struct A {\n  field b \"Nope\"\n}";
    assert_eq!(&src[span.offset as usize..(span.offset + span.len) as usize], "\"Nope\"");
    let mut out = String::new();
    miette::GraphicalReportHandler::new_themed(miette::GraphicalTheme::unicode_nocolor())
        .render_report(&mut out, &f.rendered()[0])
        .unwrap();
    assert!(out.contains("test.kdl:2:11"), "{out}");
    assert!(out.contains("korek::unknown_type"), "{out}");
}
