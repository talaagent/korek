# korek-ir

[![CI](https://github.com/talaagent/korek/actions/workflows/ci.yml/badge.svg)](https://github.com/talaagent/korek/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/korek-ir.svg)](https://crates.io/crates/korek-ir)
[![downloads](https://img.shields.io/crates/d/korek-ir.svg)](https://crates.io/crates/korek-ir)
[![docs.rs](https://docs.rs/korek-ir/badge.svg)](https://docs.rs/korek-ir)
[![lib.rs](https://img.shields.io/badge/lib.rs-korek--ir-blue.svg)](https://lib.rs/crates/korek-ir)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://github.com/talaagent/korek/blob/main/Cargo.toml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/talaagent/korek/blob/main/LICENSE)

The intermediate representation and plugin protocol of [korek](https://github.com/talaagent/korek): the
schema (types, fields, validation rules, keys, services) and the
`GenerateRequest` / `GenerateResponse` messages, as plain serde data with
minimal dependencies so generator plugins compile fast.

Part of [korek](https://github.com/talaagent/korek): schema files in, Rust, Protobuf, Zod, TypeScript and JSON Schema out.
