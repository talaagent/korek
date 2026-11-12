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

/// A driver whose built-in plugins are this package's binaries.
pub fn driver() -> Driver {
    Driver::new()
        .plugin("rust", env!("CARGO_BIN_EXE_korek-gen-rust"))
        .plugin("proto", env!("CARGO_BIN_EXE_korek-gen-proto"))
        .plugin("zod", env!("CARGO_BIN_EXE_korek-gen-zod"))
        .plugin("ts", env!("CARGO_BIN_EXE_korek-gen-ts"))
}

/// The four built-in outputs into `gen`.
pub fn all_outputs(d: Driver, gen_dir: &Path) -> Driver {
    ["rust", "proto", "zod", "ts"]
        .into_iter()
        .fold(d, |d, n| d.output(Output::new(n, gen_dir.join(n))))
}

/// Whether `prog` runs.
pub fn have(prog: &str) -> bool {
    std::process::Command::new(prog)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}
