//! The committed outputs in `examples/basic/gen` are the golden files: the
//! generators, run through the plugin protocol, must reproduce them. Run
//! with `KOREK_BLESS=1` to rewrite them after an intended change.

mod common;

use std::process::Command;

use common::{all_outputs, driver, example, have};

#[test]
fn example_outputs_match_the_golden_files() {
    let bless = std::env::var_os("KOREK_BLESS").is_some();
    let d = all_outputs(driver(), &example().join("gen")).check(!bless);
    match d.run(&example().join("schema")) {
        Ok(_) => {}
        Err(korek::Error::Stale(paths)) => panic!(
            "generated files differ from the golden files (rerun with KOREK_BLESS=1 if intended):\n{paths:#?}"
        ),
        Err(e) => panic!("{e}"),
    }
}

/// `buf lint` and `buf build` (which resolves protovalidate) accept the
/// generated Protobuf. Skipped when buf is not installed (it is in the dev
/// shell).
#[test]
fn generated_proto_passes_buf() {
    if !have("buf") {
        eprintln!("skipping: buf is not on PATH");
        return;
    }
    let dir = example().join("gen/proto");
    for args in [&["lint"][..], &["build", "-o", "/dev/null"][..]] {
        let out = Command::new("buf")
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "buf {args:?} failed:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// protoc parses the generated file without protovalidate (the
/// `protovalidate=false` option), as a second opinion. Skipped without protoc.
#[test]
fn generated_proto_without_protovalidate_passes_protoc() {
    if !have("protoc") {
        eprintln!("skipping: protoc is not on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    driver()
        .output(korek::Output::new("proto", tmp.path()).option("protovalidate", "false"))
        .lock_path(tmp.path().join("korek.lock"))
        .run(&example().join("schema"))
        .unwrap();
    let text = std::fs::read_to_string(tmp.path().join("example/v1/types.proto")).unwrap();
    assert!(!text.contains("buf.validate"), "{text}");
    let out = Command::new("protoc")
        .arg(format!("--proto_path={}", tmp.path().display()))
        .arg(format!(
            "--descriptor_set_out={}",
            tmp.path().join("out.pb").display()
        ))
        .arg("example/v1/types.proto")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
