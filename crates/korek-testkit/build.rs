//! Generates Rust for the vectors schema, so the tests run the real
//! generated `validate()`.

use std::path::PathBuf;

#[path = "src/lib.rs"]
#[allow(dead_code)]
mod vectors;

fn main() {
    println!("cargo::rerun-if-changed=vectors/rules.json");
    let schema = vectors::schema();
    let loaded = korek_core::Loader::new()
        .load_sources(
            vec![korek_core::SourceFile {
                path: "vectors.kdl".into(),
                contents: schema,
            }],
            None,
        )
        .unwrap_or_else(|f| panic!("the vectors schema does not load: {f}"));
    let (files, _) =
        korek_gen_rust::generate(&loaded.schema, &korek_gen_rust::Options::default(), &[]);
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("vectors.rs");
    std::fs::write(out, &files[0].content).unwrap_or_else(|e| panic!("writing vectors.rs: {e}"));
}
