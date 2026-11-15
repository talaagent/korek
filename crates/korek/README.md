# korek

[![CI](https://github.com/talaagent/korek/actions/workflows/ci.yml/badge.svg)](https://github.com/talaagent/korek/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/korek.svg)](https://crates.io/crates/korek)
[![downloads](https://img.shields.io/crates/d/korek.svg)](https://crates.io/crates/korek)
[![docs.rs](https://docs.rs/korek/badge.svg)](https://docs.rs/korek)
[![lib.rs](https://img.shields.io/badge/lib.rs-korek-blue.svg)](https://lib.rs/crates/korek)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://github.com/talaagent/korek/blob/main/Cargo.toml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/talaagent/korek/blob/main/LICENSE)

The library facade of [korek](https://github.com/talaagent/korek), a schema-first code generator: define
types, typed settings keys and RPC services once in KDL and generate Rust,
Protobuf (with protovalidate), Zod, TypeScript, JSON Schema and markdown
docs. It re-exports the parser (`korek-core`), the IR, the plugin protocol,
the runtime and (default `generators` feature) the first-party generators,
and has the `Driver` that the `korek` CLI runs.

```rust
use korek::{Driver, Output};

fn main() -> Result<(), korek::Error> {
    Driver::new()
        .with_builtins()
        .output(Output::new("rust", "src/generated"))
        .output(Output::new("proto", "proto"))
        .run("schema".as_ref())?;
    Ok(())
}
```

For the command line, install [`korek-cli`](https://crates.io/crates/korek-cli).
