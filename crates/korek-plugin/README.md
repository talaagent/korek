# korek-plugin

[![CI](https://github.com/talaagent/korek/actions/workflows/ci.yml/badge.svg)](https://github.com/talaagent/korek/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/korek-plugin.svg)](https://crates.io/crates/korek-plugin)
[![downloads](https://img.shields.io/crates/d/korek-plugin.svg)](https://crates.io/crates/korek-plugin)
[![docs.rs](https://docs.rs/korek-plugin/badge.svg)](https://docs.rs/korek-plugin)
[![lib.rs](https://img.shields.io/badge/lib.rs-korek--plugin-blue.svg)](https://lib.rs/crates/korek-plugin)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://github.com/talaagent/korek/blob/main/Cargo.toml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/talaagent/korek/blob/main/LICENSE)

Write a [korek](https://github.com/talaagent/korek) generator plugin. The `korek` driver sends each
`korek-gen-<name>` binary the schema IR as a JSON `GenerateRequest` on stdin
and reads the generated files back, like protoc's `protoc-gen-*`. This crate
has no parser dependency.

```rust
use korek_plugin::{File, GenerateResponse};

fn main() -> std::process::ExitCode {
    korek_plugin::run(|req| {
        let names: Vec<_> = req.schema.types.iter().map(|t| t.name.as_str()).collect();
        Ok(GenerateResponse::new(vec![File::new("types.txt", names.join("\n"))]))
    })
}
```

Name the binary `korek-gen-<name>` and run `korek gen --out <name>=DIR`.

`run` needs the default `std` feature. With `default-features = false` the
crate is `no_std` + `alloc`: the protocol types, `Encoding` and `Hook`.

Part of [korek](https://github.com/talaagent/korek): schema files in, Rust, Protobuf, Zod, TypeScript and JSON Schema out.
