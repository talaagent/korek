//! Snapshots of the diagnostics only generators and the CLI raise, as the
//! `korek` binary renders them (no color; not a terminal, so 80 columns).
//! The loader's codes are snapshotted in `korek-core`'s `tests/render.rs`.

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::Command;

/// Runs `korek` in `dir` (so paths in messages are relative) and returns its
/// stderr.
fn korek(dir: &Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_korek"))
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .env_remove("KOREK_LOG")
        .output()
        .unwrap();
    String::from_utf8(out.stderr).unwrap()
}

fn setup(schema: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("schema")).unwrap();
    std::fs::write(tmp.path().join("schema/a.kdl"), schema).unwrap();
    tmp
}

#[cfg(unix)]
fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

const SCHEMA: &str =
    "package \"t.v1\"\nstruct A {\n  field x \"string\" { validate { emoji } }\n}\n";

#[test]
fn rule_unsupported() {
    let tmp = setup(SCHEMA);
    insta::assert_snapshot!(korek(tmp.path(), &["gen", "--proto", "out", "schema"]));
}

#[test]
fn stale_output() {
    let tmp = setup(SCHEMA);
    insta::assert_snapshot!(korek(
        tmp.path(),
        &["gen", "--check", "--ts", "out", "schema"]
    ));
}

#[test]
fn plugin_not_found() {
    let tmp = setup(SCHEMA);
    insta::assert_snapshot!(korek(
        tmp.path(),
        &["gen", "--no-builtin", "--out", "nosuch=out", "schema"]
    ));
}

#[test]
fn invalid_option() {
    let tmp = setup(SCHEMA);
    insta::assert_snapshot!(korek(
        tmp.path(),
        &["gen", "--rust", "out", "--rust_opt", "nope=1", "schema"]
    ));
}

#[cfg(unix)]
#[test]
fn plugin_failed() {
    let tmp = setup(SCHEMA);
    script(tmp.path(), "fail.sh", "echo 'it broke' >&2; exit 3");
    insta::assert_snapshot!(korek(
        tmp.path(),
        &[
            "gen",
            "--out",
            "foo=out",
            "--plugin",
            "foo=./fail.sh",
            "schema"
        ]
    ));
}

#[cfg(unix)]
#[test]
fn plugin_protocol() {
    let tmp = setup(SCHEMA);
    script(tmp.path(), "bad.sh", "cat >/dev/null; echo 'not json'");
    insta::assert_snapshot!(korek(
        tmp.path(),
        &[
            "gen",
            "--out",
            "foo=out",
            "--plugin",
            "foo=./bad.sh",
            "schema"
        ]
    ));
}
