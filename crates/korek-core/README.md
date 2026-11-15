# korek-core

[![CI](https://github.com/talaagent/korek/actions/workflows/ci.yml/badge.svg)](https://github.com/talaagent/korek/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/korek-core.svg)](https://crates.io/crates/korek-core)
[![downloads](https://img.shields.io/crates/d/korek-core.svg)](https://crates.io/crates/korek-core)
[![docs.rs](https://docs.rs/korek-core/badge.svg)](https://docs.rs/korek-core)
[![lib.rs](https://img.shields.io/badge/lib.rs-korek--core-blue.svg)](https://lib.rs/crates/korek-core)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://github.com/talaagent/korek/blob/main/Cargo.toml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/talaagent/korek/blob/main/LICENSE)

The schema front end of [korek](https://github.com/talaagent/korek): loads a directory of KDL v2 schema
files, lowers them to the IR (`korek-ir`), validates names, types, rules,
defaults and examples, and assigns stable Protobuf numbers from `korek.lock`.
Diagnostics carry stable codes and source spans; the `miette` feature
renders them.

`korek_core::load(dir)` returns the IR; `korek_core::Loader` adds extensions,
a lockfile path and in-memory sources.

Part of [korek](https://github.com/talaagent/korek): schema files in, Rust, Protobuf, Zod, TypeScript and JSON Schema out.
