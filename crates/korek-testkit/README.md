# korek-testkit

[![CI](https://github.com/talaagent/korek/actions/workflows/ci.yml/badge.svg)](https://github.com/talaagent/korek/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/korek-testkit.svg)](https://crates.io/crates/korek-testkit)
[![downloads](https://img.shields.io/crates/d/korek-testkit.svg)](https://crates.io/crates/korek-testkit)
[![docs.rs](https://docs.rs/korek-testkit/badge.svg)](https://docs.rs/korek-testkit)
[![lib.rs](https://img.shields.io/badge/lib.rs-korek--testkit-blue.svg)](https://lib.rs/crates/korek-testkit)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://github.com/talaagent/korek/blob/main/Cargo.toml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/talaagent/korek/blob/main/LICENSE)

Shared validation test vectors for [korek](https://github.com/talaagent/korek) generators. Each vector is a
type, a `validate` block, and values every target must accept or reject; the
same table runs against korek's loader, its generated Rust, Zod and
Protobuf, and (through `korek_testkit::run`) any third-party generator.

Part of [korek](https://github.com/talaagent/korek): schema files in, Rust, Protobuf, Zod, TypeScript and JSON Schema out.
