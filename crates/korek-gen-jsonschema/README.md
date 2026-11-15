# korek-gen-jsonschema

[![CI](https://github.com/talaagent/korek/actions/workflows/ci.yml/badge.svg)](https://github.com/talaagent/korek/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/korek-gen-jsonschema.svg)](https://crates.io/crates/korek-gen-jsonschema)
[![downloads](https://img.shields.io/crates/d/korek-gen-jsonschema.svg)](https://crates.io/crates/korek-gen-jsonschema)
[![docs.rs](https://docs.rs/korek-gen-jsonschema/badge.svg)](https://docs.rs/korek-gen-jsonschema)
[![lib.rs](https://img.shields.io/badge/lib.rs-korek--gen--jsonschema-blue.svg)](https://lib.rs/crates/korek-gen-jsonschema)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://github.com/talaagent/korek/blob/main/Cargo.toml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/talaagent/korek/blob/main/LICENSE)

The JSON Schema (2020-12) generator of [korek](https://github.com/talaagent/korek): one `schema.json`
with a `$defs` entry per type and the settings keys, or one self-contained
file per type (`per_type=true`), for example for MCP tool input schemas.

Part of [korek](https://github.com/talaagent/korek): schema files in, Rust, Protobuf, Zod, TypeScript and JSON Schema out.
