//! The `korek` binary: `gen`, `--check`, errors and options.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::Path;
use std::process::Command;

use common::example;

fn korek(args: &[&str]) -> std::process::Output {
    let bin_dir = Path::new(env!("CARGO_BIN_EXE_korek")).parent().unwrap();
    let path = std::env::join_paths(core::iter::once(bin_dir.to_path_buf()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    Command::new(env!("CARGO_BIN_EXE_korek"))
        .args(args)
        .env("PATH", path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

fn copy_schema(to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(example().join("schema")).unwrap() {
        let p = e.unwrap().path();
        std::fs::copy(&p, to.join(p.file_name().unwrap())).unwrap();
    }
}

#[test]
fn gen_then_check() {
    let tmp = tempfile::tempdir().unwrap();
    let schema = tmp.path().join("schema");
    copy_schema(&schema);
    let gen_dir = tmp.path().join("gen");
    let s = |p: &Path| p.to_str().unwrap().to_owned();
    let (rust, ts, schema_s) = (s(&gen_dir.join("rust")), s(&gen_dir.join("ts")), s(&schema));
    let args = ["gen", "--rust", &rust, "--ts", &ts, &schema_s];
    // --check before generating: stale.
    let mut check = vec!["gen", "--check"];
    check.extend_from_slice(&args[1..]);
    let out = korek(&check);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("out of date"));
    // Generate, then --check passes.
    let out = korek(&args);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(gen_dir.join("rust/types.rs").is_file());
    assert!(korek(&check).status.success());
    // Edit the schema: stale again.
    let types = schema.join("types.kdl");
    let mut text = std::fs::read_to_string(&types).unwrap();
    text.push_str("\nstruct Extra { field x \"string\" }\n");
    std::fs::write(&types, text).unwrap();
    let out = korek(&check);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("types.rs"));
}

#[test]
fn schema_errors_are_rendered_with_spans() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("a.kdl"),
        "struct A {\n  field b \"Bee\"\n}\nstruct Bee2 {}\n",
    )
    .unwrap();
    let out = korek(&["check", tmp.path().to_str().unwrap()]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("korek::unknown_type"), "{err}");
    assert!(err.contains("a.kdl:2:11"), "{err}");
    assert!(err.contains("did you mean `Bee2`?"), "{err}");
}

#[test]
fn plugin_options_and_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let schema = tmp.path().join("schema");
    copy_schema(&schema);
    let rust = tmp.path().join("rust");
    let out = korek(&[
        "gen",
        "--rust",
        rust.to_str().unwrap(),
        "--rust_opt",
        "file=generated.rs",
        schema.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(rust.join("generated.rs").is_file());
    // An unknown option is the plugin's error.
    let out = korek(&[
        "gen",
        "--rust",
        rust.to_str().unwrap(),
        "--rust_opt",
        "nope=1",
        schema.to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown option `nope`"));
    // A missing plugin names the binary.
    let out = korek(&["gen", "--out", "nosuch=x", schema.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("korek-gen-nosuch"));
}

#[test]
fn ir_prints_the_schema() {
    let out = korek(&["ir", example().join("schema").to_str().unwrap()]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["package"], "example.v1");
}

/// Every line `--message-format json` prints is a diagnostic object; errors
/// without a source location have a `null` file, line and column.
fn json_lines(out: &std::process::Output) -> Vec<serde_json::Value> {
    assert!(
        out.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}")))
        .collect()
}

#[track_caller]
fn unlocated(out: &std::process::Output, code: &str) {
    assert!(!out.status.success());
    let lines = json_lines(out);
    let d = lines
        .iter()
        .find(|d| d["code"] == code)
        .unwrap_or_else(|| panic!("no {code} in {lines:?}"));
    for k in ["file", "line", "col"] {
        assert!(d[k].is_null(), "{k} in {d}");
    }
    assert_eq!(d["severity"], "error");
    assert_eq!(d["labels"], serde_json::json!([]));
}

#[test]
fn json_errors_without_a_location() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("missing");
    let missing = missing.to_str().unwrap();
    let j = ["--message-format", "json"];
    unlocated(
        &korek(&[&["check"], &j[..], &[missing]].concat()),
        "korek::io",
    );
    let schema = tmp.path().join("schema");
    copy_schema(&schema);
    let schema = schema.to_str().unwrap();
    let gen_args = |extra: &[&'static str]| -> Vec<String> {
        ["gen", "--message-format", "json"]
            .iter()
            .chain(extra)
            .map(|s| (*s).to_owned())
            .chain([schema.to_owned()])
            .collect()
    };
    let run = |a: Vec<String>| korek(&a.iter().map(String::as_str).collect::<Vec<_>>());
    unlocated(
        &run(gen_args(&["--out", "nosuch=x"])),
        "korek::plugin_not_found",
    );
    unlocated(&run(gen_args(&[])), "korek::invalid_option");
    let rust = tmp.path().join("rust");
    let rust: &'static str = Box::leak(rust.to_str().unwrap().to_owned().into_boxed_str());
    unlocated(
        &run(gen_args(&["--rust", rust, "--rust_opt", "nope=1"])),
        "korek::invalid_option",
    );
    // An unreadable lockfile (here a directory) has no span either.
    let lock: &'static str = Box::leak(missing.to_owned().into_boxed_str());
    std::fs::create_dir(lock).unwrap();
    let out = run(gen_args(&["--rust", rust, "--lock", lock]));
    assert!(!out.status.success());
    for d in json_lines(&out) {
        assert!(d["file"].is_null(), "{d}");
    }
    // A located error has all three.
    std::fs::write(
        tmp.path().join("schema/types.kdl"),
        "struct A {\n  field b \"Nope\"\n}\n",
    )
    .unwrap();
    let lines = json_lines(&run(gen_args(&["--rust", rust])));
    let d = lines
        .iter()
        .find(|d| d["message"] == "unknown type `Nope`")
        .unwrap();
    assert_eq!(d["file"], "types.kdl");
    assert_eq!((d["line"].as_u64(), d["col"].as_u64()), (Some(2), Some(11)));
}

/// The input-hash cache: a second identical run skips loading and every
/// plugin; editing the schema, changing options or deleting an output
/// regenerates.
#[test]
fn cache_skips_unchanged_runs() {
    use korek::{Driver, Output, Via};
    let tmp = tempfile::tempdir().unwrap();
    let schema = tmp.path().join("schema");
    copy_schema(&schema);
    let cache = tmp.path().join("cache");
    let driver = |opt: &str| {
        Driver::new()
            .with_builtins()
            .no_path_lookup(true)
            .cache(Some(cache.clone()))
            .output(Output::new("rust", tmp.path().join("rust")))
            .output(Output::new("ts", tmp.path().join("ts")).option("file", opt))
    };
    let vias = |r: &korek::Report| r.plugins.iter().map(|p| p.1).collect::<Vec<_>>();
    let first = driver("a.ts").run(&schema).unwrap();
    assert_eq!(vias(&first), [Via::Builtin, Via::Builtin]);
    let second = driver("a.ts").run(&schema).unwrap();
    assert_eq!(vias(&second), [Via::Cached, Via::Cached]);
    assert_eq!(second.stale().count(), 0);
    assert_eq!(second.files.len(), first.files.len());
    // An option changes one output's key.
    let r = driver("b.ts").run(&schema).unwrap();
    assert_eq!(vias(&r), [Via::Cached, Via::Builtin]);
    // A deleted output is regenerated.
    std::fs::remove_file(tmp.path().join("rust/types.rs")).unwrap();
    let r = driver("b.ts").run(&schema).unwrap();
    assert_eq!(vias(&r), [Via::Builtin, Via::Cached]);
    assert!(tmp.path().join("rust/types.rs").is_file());
    // A schema edit misses everything.
    let types = schema.join("types.kdl");
    let text = std::fs::read_to_string(&types).unwrap();
    std::fs::write(&types, text + "\nstruct Extra { field x \"string\" }\n").unwrap();
    let r = driver("b.ts").run(&schema).unwrap();
    assert_eq!(vias(&r), [Via::Builtin, Via::Builtin]);
    assert!(r.stale().count() >= 2);
    assert_eq!(
        vias(&driver("b.ts").run(&schema).unwrap()),
        [Via::Cached, Via::Cached]
    );
}
