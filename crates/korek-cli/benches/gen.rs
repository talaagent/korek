//! Benchmarks on a large synthetic schema (`korek_testkit::synthetic`,
//! about 2000 types and 500 keys): loading, the IR's JSON encoding, each
//! generator, and whole `korek gen` runs with builtin and subprocess
//! plugins. `cargo bench -p korek-cli`; numbers in BENCHMARKS.md.

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::sync::LazyLock;

use divan::Bencher;
use korek::{Driver, Output};
use korek_core::{Loader, SourceFile};
use korek_ir::{GenerateRequest, Schema};
use korek_testkit::synthetic::{Size, schema};

fn main() {
    divan::main();
}

static SOURCE: LazyLock<String> = LazyLock::new(|| schema(Size::LARGE));

static SCHEMA: LazyLock<Schema> = LazyLock::new(|| load(&SOURCE));

fn load(src: &str) -> Schema {
    Loader::new()
        .load_sources(
            vec![SourceFile {
                path: "bench.kdl".into(),
                contents: src.to_owned(),
            }],
            None,
        )
        .unwrap()
        .schema
}

const GENERATORS: &[&str] = &["rust", "proto", "zod", "ts", "jsonschema"];

type Plugin = fn(&GenerateRequest) -> Result<korek_ir::GenerateResponse, korek_ir::Diagnostic>;

fn plugin(name: &str) -> Plugin {
    match name {
        "rust" => korek_gen_rust::plugin,
        "proto" => korek_gen_proto::plugin,
        "zod" => korek_gen_zod::plugin,
        "ts" => korek_gen_ts::plugin,
        _ => korek_gen_jsonschema::plugin,
    }
}

/// Parse, lower, validate, check defaults and examples, number.
#[divan::bench(sample_count = 30)]
fn load_and_validate(b: Bencher) {
    b.bench(|| load(&SOURCE));
}

/// The `GenerateRequest` a subprocess plugin receives, as JSON.
#[divan::bench]
fn encode_request(b: Bencher) {
    let req = GenerateRequest::new("x", SCHEMA.clone());
    b.bench(|| serde_json::to_vec(&req).unwrap());
}

#[divan::bench(args = GENERATORS)]
fn generator(b: Bencher, name: &str) {
    let req = GenerateRequest::new(name, SCHEMA.clone());
    let f = plugin(name);
    b.bench(|| f(&req).unwrap());
}

fn driver(dir: &Path, subprocess: bool) -> Driver {
    let mut d = Driver::new().with_builtins().no_path_lookup(true);
    for name in GENERATORS {
        if subprocess {
            let exe = match *name {
                "rust" => env!("CARGO_BIN_EXE_korek-gen-rust"),
                "proto" => env!("CARGO_BIN_EXE_korek-gen-proto"),
                "zod" => env!("CARGO_BIN_EXE_korek-gen-zod"),
                "ts" => env!("CARGO_BIN_EXE_korek-gen-ts"),
                _ => env!("CARGO_BIN_EXE_korek-gen-jsonschema"),
            };
            d = d.plugin(*name, exe);
        }
        d = d.output(Output::new(*name, dir.join("out").join(name)));
    }
    d
}

/// `korek gen` with all five generators, outputs already up to date.
#[divan::bench(args = [false, true], sample_count = 20)]
fn full_gen(b: Bencher, subprocess: bool) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("schema")).unwrap();
    std::fs::write(dir.path().join("schema/bench.kdl"), &*SOURCE).unwrap();
    let d = driver(dir.path(), subprocess);
    d.run(&dir.path().join("schema")).unwrap();
    b.bench(|| d.run(&dir.path().join("schema")).unwrap());
}
