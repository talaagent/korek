//! The committed outputs in `examples/basic/gen` are the golden files: the
//! generators, run through the plugin protocol, must reproduce them. Run
//! with `KOREK_BLESS=1` to rewrite them after an intended change.

mod common;

use std::process::Command;

use common::{all_outputs, builtin_driver, driver, example, have};

#[test]
fn example_outputs_match_the_golden_files() {
    let bless = std::env::var_os("KOREK_BLESS").is_some();
    let d = all_outputs(driver(), &example().join("gen")).check(!bless);
    match d.run(&example().join("schema")) {
        Ok(_) => {}
        Err(e) if e.has(korek::korek_core::codes::STALE_OUTPUT) => panic!(
            "generated files differ from the golden files (rerun with KOREK_BLESS=1 if intended):\n{e}"
        ),
        Err(e) => panic!("{e}"),
    }
}

/// The Zod and TS outputs for the shared vectors (`ts/vectors.test.ts`).
#[test]
fn vector_outputs_match_the_golden_files() {
    let kit = common::root().join("crates/korek-testkit");
    let bless = std::env::var_os("KOREK_BLESS").is_some();
    let d = builtin_driver()
        .output(korek::Output::new("zod", kit.join("gen/zod")))
        .output(korek::Output::new("ts", kit.join("gen/ts")))
        .lock_path(kit.join("schema/korek.lock"))
        .check(!bless);
    if let Err(e) = d.run(&kit.join("schema")) {
        panic!("{e} (rerun with KOREK_BLESS=1 if intended)");
    }
}

/// The in-process builtins and the plugin binaries produce the same files.
#[test]
fn builtin_and_subprocess_agree() {
    let tmp = tempfile::tempdir().unwrap();
    let (a, b) = (tmp.path().join("sub"), tmp.path().join("bi"));
    let ra = all_outputs(driver(), &a)
        .lock_path(tmp.path().join("a.lock"))
        .run(&example().join("schema"))
        .unwrap();
    let rb = all_outputs(builtin_driver(), &b)
        .lock_path(tmp.path().join("b.lock"))
        .run(&example().join("schema"))
        .unwrap();
    assert!(ra.plugins.iter().all(|p| p.1 == korek::Via::Subprocess));
    assert!(rb.plugins.iter().all(|p| p.1 == korek::Via::Builtin));
    assert_eq!(ra.files.len(), rb.files.len());
    for (x, y) in ra.files.iter().zip(&rb.files) {
        assert_eq!(
            x.content,
            y.content,
            "{} differs from {}",
            x.path.display(),
            y.path.display()
        );
    }
    assert_eq!(ra.warnings, rb.warnings);
}

/// Zod output never infers types; every schema is checked against one.
#[test]
fn zod_never_infers() {
    for f in ["zod/zod.ts", "zod-inline/zod.ts"] {
        let text = std::fs::read_to_string(example().join("gen").join(f)).unwrap();
        for banned in ["z.infer", "z.input", "z.output"] {
            assert!(!text.contains(banned), "{f} contains {banned}");
        }
        let schemas = text.matches("Schema = ").count();
        assert_eq!(
            schemas,
            text.matches("satisfies z.ZodType<").count(),
            "{f}: a schema without satisfies"
        );
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
        .output(
            korek::Output::new("proto", tmp.path())
                .option("protovalidate", "false")
                .option("buf_yaml", "false"),
        )
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
