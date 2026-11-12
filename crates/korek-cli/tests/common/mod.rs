//! Shared test helpers.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use korek::{Driver, Output};

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn example() -> PathBuf {
    root().join("examples/basic")
}

/// A driver that runs this package's plugin binaries as subprocesses.
pub fn driver() -> Driver {
    Driver::new()
        .plugin("rust", env!("CARGO_BIN_EXE_korek-gen-rust"))
        .plugin("proto", env!("CARGO_BIN_EXE_korek-gen-proto"))
        .plugin("zod", env!("CARGO_BIN_EXE_korek-gen-zod"))
        .plugin("ts", env!("CARGO_BIN_EXE_korek-gen-ts"))
        .plugin("jsonschema", env!("CARGO_BIN_EXE_korek-gen-jsonschema"))
        .plugin("docs", env!("CARGO_BIN_EXE_korek-gen-docs"))
}

/// A driver that runs the generators in-process.
pub fn builtin_driver() -> Driver {
    Driver::new().with_builtins().no_path_lookup(true)
}

/// Every first-party output into `gen`, plus Zod with inline types.
pub fn all_outputs(d: Driver, gen_dir: &Path) -> Driver {
    ["rust", "proto", "zod", "ts", "jsonschema", "docs"]
        .into_iter()
        .fold(d, |d, n| d.output(Output::new(n, gen_dir.join(n))))
        .output(Output::new("zod", gen_dir.join("zod-inline")).option("types", "inline"))
        .output(Output::new("docs", gen_dir.join("docs-split")).option("split", "true"))
}

/// Whether `prog` runs.
pub fn have(prog: &str) -> bool {
    std::process::Command::new(prog)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}
