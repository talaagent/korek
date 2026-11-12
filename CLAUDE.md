# korek

KDL v2 type definitions → Rust, Protobuf, Zod, TypeScript.

- Dev shell: `nix develop` (Rust from `rust-toolchain.toml` via rust-overlay, bun, buf, protoc).
- Gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
