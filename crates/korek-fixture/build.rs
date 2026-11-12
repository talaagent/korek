//! Generates Rust for `schema/wire.kdl` with `rust_duration` on.

use std::path::PathBuf;

fn main() {
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
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("wire.rs");
    std::fs::write(out, &files[0].content).unwrap_or_else(|e| panic!("writing wire.rs: {e}"));
}
