# korek-gen-rust

[![CI](https://github.com/talaagent/korek/actions/workflows/ci.yml/badge.svg)](https://github.com/talaagent/korek/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/korek-gen-rust.svg)](https://crates.io/crates/korek-gen-rust)
[![downloads](https://img.shields.io/crates/d/korek-gen-rust.svg)](https://crates.io/crates/korek-gen-rust)
[![docs.rs](https://docs.rs/korek-gen-rust/badge.svg)](https://docs.rs/korek-gen-rust)
[![lib.rs](https://img.shields.io/badge/lib.rs-korek--gen--rust-blue.svg)](https://lib.rs/crates/korek-gen-rust)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://github.com/talaagent/korek/blob/main/Cargo.toml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/talaagent/korek/blob/main/LICENSE)

The Rust generator of [korek](https://github.com/talaagent/korek): serde types, a `validate()` per type
(on `korek-runtime`), `Default` impls and typed settings key constants
(`keys::SESSION_STEER_ENABLED: Key<bool>`). The output is one file meant to
be `include!`d; it needs `serde` and `korek-runtime`.

Part of [korek](https://github.com/talaagent/korek): schema files in, Rust, Protobuf, Zod, TypeScript and JSON Schema out.
