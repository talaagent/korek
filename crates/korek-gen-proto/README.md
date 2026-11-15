# korek-gen-proto

[![CI](https://github.com/talaagent/korek/actions/workflows/ci.yml/badge.svg)](https://github.com/talaagent/korek/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/korek-gen-proto.svg)](https://crates.io/crates/korek-gen-proto)
[![downloads](https://img.shields.io/crates/d/korek-gen-proto.svg)](https://crates.io/crates/korek-gen-proto)
[![docs.rs](https://docs.rs/korek-gen-proto/badge.svg)](https://docs.rs/korek-gen-proto)
[![lib.rs](https://img.shields.io/badge/lib.rs-korek--gen--proto-blue.svg)](https://lib.rs/crates/korek-gen-proto)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://github.com/talaagent/korek/blob/main/Cargo.toml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/talaagent/korek/blob/main/LICENSE)

The Protobuf generator of [korek](https://github.com/talaagent/korek): proto3 messages, enums and
`service`s for gRPC / ConnectRPC, with [protovalidate](https://protovalidate.com)
rules for every validation rule it can express, stable field numbers from
`korek.lock`, and a `buf.yaml`.

Part of [korek](https://github.com/talaagent/korek): schema files in, Rust, Protobuf, Zod, TypeScript and JSON Schema out.
