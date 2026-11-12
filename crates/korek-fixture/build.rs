//! Generates Rust for `schema/wire.kdl` with `rust_duration` on, and for
//! `schema/korek` (korek's own IR and protocol, `tests/selfhost.rs`).

use std::path::{Path, PathBuf};

fn main() {
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default());
    let path = "schema/wire.kdl";
    println!("cargo::rerun-if-changed={path}");
    let contents = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    let loaded = korek_core::Loader::new()
        .load_sources(
            vec![korek_core::SourceFile {
                path: path.into(),
                contents,
            }],
            None,
        )
        .unwrap_or_else(|f| panic!("{path} does not load: {f}"));
    let mut opts = korek_gen_rust::Options::default();
    opts.duration = true;
    let (files, _) = korek_gen_rust::generate(&loaded.schema, &opts, &[]);
    std::fs::write(out.join("wire.rs"), &files[0].content)
        .unwrap_or_else(|e| panic!("writing wire.rs: {e}"));

    let dir = Path::new("../../schema/korek");
    println!("cargo::rerun-if-changed={}", dir.display());
    let loaded = korek_core::Loader::new()
        .load(dir)
        .unwrap_or_else(|f| panic!("schema/korek does not load: {f}"));
    let (files, _) =
        korek_gen_rust::generate(&loaded.schema, &korek_gen_rust::Options::default(), &[]);
    std::fs::write(out.join("selfhost.rs"), &files[0].content)
        .unwrap_or_else(|e| panic!("writing selfhost.rs: {e}"));
}
