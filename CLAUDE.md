# korek

Schema files (KDL v2) → validated IR → generator plugins (Rust, Protobuf, Zod, TS).

## Layout

- `crates/korek-core`: parse (`parse.rs`), IR (`ir.rs`, the stable plugin-facing types), validation
  (`validate.rs`), typed values (`value.rs`), lockfile numbering (`lock.rs`), extensions (`ext.rs`),
  diagnostics (`diag.rs`). No CLI or I/O beyond reading the schema dir.
- `crates/korek-plugin`: the plugin protocol (`GenerateRequest`/`GenerateResponse`, JSON), `run`, `Hook`.
- `crates/korek-gen-{rust,proto,zod,ts}`: one library per generator: `generate(&schema, &opts, hooks)`
  and `plugin(&req)`.
- `crates/korek`: the `Driver` library, the `korek` CLI, and the `korek-gen-*` binaries (`src/bin/`, thin
  mains over the generator crates; they live here so tests get `CARGO_BIN_EXE_*`).
- `crates/korek-fixture`: `include!`s the generated Rust for `examples/basic` so it is compiled and tested.
- `examples/basic`: the example schema; `gen/` holds the golden outputs (`crates/korek/tests/golden.rs`).
- `examples/plugin-example`: a third-party plugin and a driver test for it.
- `ts/`: `bun test` for the Zod output (root `package.json`, `tsconfig.json`).

## Gate

```sh
nix develop -c cargo fmt --all --check
nix develop -c cargo clippy --workspace --all-targets -- -D warnings
nix develop -c cargo test --workspace
nix develop -c sh -c 'bun install && bun test && bun run typecheck'
```

## Conventions

- After changing a generator, regenerate the goldens with `KOREK_BLESS=1 cargo test -p korek --test
  golden` and review the diff in `examples/basic/gen`.
- IR changes are protocol changes: keep structs `#[non_exhaustive]`, add fields with serde defaults,
  bump `korek_plugin::PROTOCOL_VERSION` on breaking ones.
- Every diagnostic has a stable `korek::<code>` and a span when there is a source location; add a test
  in `crates/korek-core/tests/load.rs` for new errors.
- Generated code must pass the downstream tools: Rust compiles (fixture), proto passes `buf lint` and
  `buf build`, Zod passes `bun test` and `tsc`.
- No "kdl" in crate, binary or module names; KDL is only the input format.
