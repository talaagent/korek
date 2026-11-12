//! Snapshots of every load-time diagnostic as miette renders it (no color,
//! 80 columns). The codes only a generator or the CLI raises are
//! snapshotted in `korek-cli`'s `tests/render.rs`.

#![cfg(feature = "miette")]
#![allow(clippy::unwrap_used)]

use korek_core::{Failure, Loader, SourceFile, codes};

/// Codes raised outside the loader.
const ELSEWHERE: &[&str] = &[
    codes::RULE_UNSUPPORTED,
    codes::PLUGIN_FAILED,
    codes::PLUGIN_PROTOCOL,
    codes::PLUGIN_NOT_FOUND,
    codes::INVALID_OPTION,
    codes::STALE_OUTPUT,
];

/// `(code, schema)`; the schema of `id_taken` and `renumbered` is loaded
/// against the lock of `struct A { field x "string" }`.
const CASES: &[(&str, &str)] = &[
    (codes::SYNTAX, "struct A {\n  field x \"string\"\n"),
    (codes::UNKNOWN_NODE, "struct A {\n  fild x \"string\"\n}"),
    (
        codes::UNKNOWN_PROPERTY,
        "struct A {\n  field x \"string\" opt=#true\n}",
    ),
    (
        codes::UNKNOWN_TYPE,
        "struct A {\n  field b \"Strng\"\n}\nstruct String2 {}",
    ),
    (
        codes::UNKNOWN_RULE,
        "struct A {\n  field x \"string\" { validate { emial } }\n}",
    ),
    (codes::DUPLICATE_NAME, "struct A {}\nenum A { variant x }"),
    (
        codes::NAME_CLASH,
        "struct SessionKeys {}\nkeys \"session\" {}",
    ),
    (codes::INVALID_NAME, "struct lower_case {}"),
    (
        codes::INVALID_TYPE,
        "struct A {\n  field x \"map<i64,string>\"\n}",
    ),
    (
        codes::INVALID_VALUE,
        "struct A {}\nservice S {\n  rpc Get input=\"A\" output=\"A\" idempotency=\"sometimes\"\n}",
    ),
    (codes::MISSING_VALUE, "struct A {\n  field x\n}"),
    (
        codes::INVALID_DEFAULT,
        "keys \"s\" {\n  key \"a\" \"u64\" default=-1\n}",
    ),
    (
        codes::INVALID_EXAMPLE,
        "struct A {\n  field x \"string\" { example 5 }\n}",
    ),
    (
        codes::INVALID_RULE,
        "struct A {\n  field a \"bool\" { validate { gte 1 } }\n}",
    ),
    (
        codes::INVALID_UNION,
        "union U tag=\"t\" {\n  variant a \"string\"\n}",
    ),
    (
        codes::INVALID_RPC,
        "enum E { variant x }\nservice S {\n  rpc Do input=\"E\" output=\"E\"\n}",
    ),
    (
        codes::CYCLE,
        "struct A { field b \"B\" }\nstruct B { field a \"list<A>\" }",
    ),
    (codes::EMPTY, "enum E {}"),
    (
        codes::INVALID_ID,
        "struct A {\n  field x \"string\" id=0\n}",
    ),
    (
        codes::DUPLICATE_ID,
        "struct A {\n  field x \"string\" id=2\n  field y \"string\" id=2\n}",
    ),
    (
        codes::ID_TAKEN,
        "struct A {\n  field x \"string\"\n  field y \"string\" id=1\n}",
    ),
    (
        codes::RENUMBERED,
        "struct A {\n  field x \"string\" id=7\n}",
    ),
];

fn src(path: &str, contents: &str) -> SourceFile {
    SourceFile {
        path: path.into(),
        contents: contents.into(),
    }
}

fn base_lock() -> SourceFile {
    let l = Loader::new()
        .load_sources(vec![src("a.kdl", "struct A { field x \"string\" }")], None)
        .unwrap();
    src("korek.lock", &l.lock.render())
}

fn render(f: &Failure) -> String {
    let h = miette::GraphicalReportHandler::new_themed(miette::GraphicalTheme::unicode_nocolor())
        .with_links(false)
        .with_width(80);
    let mut out = String::new();
    for r in f.rendered() {
        h.render_report(&mut out, &r).unwrap();
    }
    out
}

#[test]
fn every_load_code_renders() {
    for (code, schema) in CASES {
        let lock = [codes::ID_TAKEN, codes::RENUMBERED]
            .contains(code)
            .then(base_lock);
        let f = Loader::new()
            .load_sources(vec![src("test.kdl", schema)], lock)
            .expect_err(code);
        assert!(f.has(code), "expected {code}, got:\n{f}");
        let name = code.strip_prefix("korek::").unwrap();
        insta::assert_snapshot!(name, render(&f), schema);
    }
}

#[test]
fn lockfile_and_io_render() {
    let bad = src("korek.lock", "struct \"A\" {\n  item \"x\"\n}\n");
    let f = Loader::new()
        .load_sources(vec![src("test.kdl", "struct A {}")], Some(bad))
        .unwrap_err();
    assert!(f.has(codes::LOCKFILE), "{f}");
    insta::assert_snapshot!("lockfile", render(&f));
    let f = Loader::new()
        .load(std::path::Path::new("does/not/exist"))
        .unwrap_err();
    assert!(f.has(codes::IO), "{f}");
    insta::assert_snapshot!("io", render(&f));
}

#[test]
fn every_code_has_a_snapshot() {
    let mut covered: Vec<&str> = CASES.iter().map(|(c, _)| *c).collect();
    covered.extend([codes::LOCKFILE, codes::IO]);
    covered.extend(ELSEWHERE);
    for (code, _, _) in codes::ALL {
        assert!(covered.contains(code), "no snapshot for {code}");
    }
}
