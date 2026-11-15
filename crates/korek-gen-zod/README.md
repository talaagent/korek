# korek-gen-zod

[![CI](https://github.com/talaagent/korek/actions/workflows/ci.yml/badge.svg)](https://github.com/talaagent/korek/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/korek-gen-zod.svg)](https://crates.io/crates/korek-gen-zod)
[![downloads](https://img.shields.io/crates/d/korek-gen-zod.svg)](https://crates.io/crates/korek-gen-zod)
[![docs.rs](https://docs.rs/korek-gen-zod/badge.svg)](https://docs.rs/korek-gen-zod)
[![lib.rs](https://img.shields.io/badge/lib.rs-korek--gen--zod-blue.svg)](https://lib.rs/crates/korek-gen-zod)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://github.com/talaagent/korek/blob/main/Cargo.toml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/talaagent/korek/blob/main/LICENSE)

The Zod v4 generator of [korek](https://github.com/talaagent/korek): a schema per type, each
`satisfies z.ZodType<T>` against the TypeScript types from `korek-gen-ts`,
validation rules as Zod checks, and a typed `keys` record.

Part of [korek](https://github.com/talaagent/korek): schema files in, Rust, Protobuf, Zod, TypeScript and JSON Schema out.
