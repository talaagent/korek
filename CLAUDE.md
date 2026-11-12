# korek

Schema files (KDL v2) → validated IR → generator plugins (Rust, Protobuf, Zod, TS).

## Layout

- `crates/korek-ir`: the IR (`ir.rs`) and plugin protocol (`protocol.rs`) as pure serde data (no logic;
  helpers in `impls.rs`), diagnostic codes (`codes.rs`, mirrored by `docs/errors.md`), typed values.
- `crates/korek-core`: parse (`parse.rs`), validation (`validate.rs`), rule evaluation for defaults and
  examples (`check.rs`), lockfile numbering (`lock.rs`), extensions (`ext.rs`), sources (`source.rs`, a shim
  until talaagent/kdl-serde's `kdl-load` lands), diagnostics (`diag.rs`; miette only behind `miette`).
- `crates/korek-plugin`: `run`, `Encoding`, `Hook`; depends on korek-ir only.
- `crates/korek-gen-common`: shared generator helpers. `crates/korek-gen-{rust,proto,zod,ts,jsonschema,docs}`:
  `generate(&schema, &opts, hooks)` and `plugin(&req)`. Zod builds on the TS crate.
- `crates/korek-runtime`: what generated Rust uses (`Validate`, `Key<T>`, formats, base64, time).
- `crates/korek`: the facade and `Driver`. `crates/korek-cli`: the `korek` binary (`builtin` feature links
  the generators) and the `korek-gen-*` binaries (`src/bin/`, so tests get `CARGO_BIN_EXE_*`).
- `crates/korek-testkit`: shared rule vectors (`vectors/rules.json`) run against the loader, generated Rust
  (build script), generated Zod (`ts/vectors.test.ts`) and generated Protobuf under protovalidate-es
  (`ts/protovalidate.test.ts`, descriptors from `buf build` of `gen/proto`).
- `crates/korek-fixture`: `include!`s the generated Rust for `examples/basic` so it is compiled and tested;
  its `build.rs` also generates `schema/korek` (see Self-hosting).
- `schema/korek`: korek's own IR (`ir.kdl`) and plugin protocol (`protocol.kdl`) as a korek schema.
- `examples/basic`: the example schema; `gen/` holds the golden outputs (`crates/korek/tests/golden.rs`).
- `examples/plugin-example`: a third-party plugin and a driver test for it.
- `ts/`: `bun test` for the Zod output (root `package.json`, `tsconfig.json`).

## Gate

```sh
nix develop -c cargo fmt --all --check
nix develop -c cargo clippy --workspace --all-targets -- -D warnings
nix develop -c cargo test --workspace
nix develop -c cargo test -p korek-core --features miette
nix develop -c sh -c 'bun install && bun test && bun run typecheck'
```

## Conventions

- After changing a generator, regenerate the goldens with `KOREK_BLESS=1 cargo test -p korek-cli --test
  golden` and review the diff in `examples/basic/gen` and `crates/korek-testkit/gen`.
- New rule: add it to `RuleKind`, the parser, `validate.rs` applicability, `check.rs`, every generator,
  `VALIDATION.md`, and a vector in `crates/korek-testkit/vectors/rules.json`.
- IR and protocol types stay pure data using only what korek can express (structs, enums, tagged unions,
  lists, maps, optionals, primitives), so they can later be generated from `schema/korek`.
- Library crates use `thiserror`; `anyhow` only in the CLI and tests. No miette in korek-ir/korek-plugin.
- KDL written back by korek (none yet; a future `korek fmt`) must keep `"""` multiline strings as written
  (kdl-rs collapses them, kdl-org/kdl-rs#77); use kdl-serde's `kdl-pretty` and test the round trip.
- IR changes are protocol changes: keep structs `#[non_exhaustive]`, add fields with serde defaults,
  bump `korek_plugin::PROTOCOL_VERSION` on breaking ones.
- Every diagnostic has a stable `korek::<code>` and a span when there is a source location; add a test
  in `crates/korek-core/tests/load.rs` for new errors. New codes also get an insta snapshot of their
  rendering: `crates/korek-core/tests/render.rs` (loader codes) or `crates/korek-cli/tests/render.rs`
  (generator/CLI codes); accept with `INSTA_UPDATE=always` (or `cargo insta review`) and read the `.snap`.
- Generated code must pass the downstream tools: Rust compiles (fixture), proto passes `buf lint` and
  `buf build`, Zod passes `bun test` and `tsc`.
- No "kdl" in crate, binary or module names; KDL is only the input format.
- Changing an IR or protocol type means changing `schema/korek` too; `crates/korek-fixture/tests/selfhost.rs`
  fails when they disagree (new `RuleKind`s also go in its `every_rule()`).

## Self-hosting

`schema/korek` describes `korek_ir::ir` and `korek_ir::protocol` with the same type, field and variant names
and the same JSON. `crates/korek-fixture/tests/selfhost.rs` serializes real IR (examples/basic, the rule
vectors, schema/korek itself, every rule kind and type) with korek-ir, deserializes it through the Rust
generated from `schema/korek` and checks the JSON comes back identical (and back into korek-ir equal).
Where korek cannot say what korek-ir says, the schema uses the closest form with the same JSON:

- `#[serde(default, skip_serializing_if = …)]` fields are `optional=#true` (`Option<Vec<T>>`, `Option<bool>`
  where korek-ir has `Vec<T>`, `bool`): korek has no "default, omitted when default".
- Externally tagged enums (`Type`, `TypeKind`) are untagged unions: an enum for the unit variants
  (`Primitive`) and one-field structs for the others (`ListType { list }`, `EnumKind { enum }`).
- Recursion (`Type` inside `list<T>`/`map<string,T>`, nested `Validate` in `items`/`keys`/`values`) is
  `json`: korek rejects recursive types (`korek::cycle`).
- Unit rule variants (`finite`, `unique`, …) carry an empty struct (`NoOperand`); variants with the same
  operands share a struct (`CountOperand`), so the generated enum has newtype variants.
- `Box`, `Copy`/`Eq`/`Hash` derives, `#[non_exhaustive]` and the hand-written helpers (`impls.rs`) have no
  counterpart.

Bootstrap plan, to replace the hand-written types:

1. Close the gaps above: recursive types (Rust `Box` on cycles, Zod `z.lazy`), a "default, omitted when
   default" field mode, and externally tagged unions (or accept the untagged form in the IR).
2. Generate `korek-ir`'s types from `schema/korek` with korek-gen-rust and diff them against `ir.rs`/
   `protocol.rs` (the round-trip test keeps the JSON in step meanwhile).
3. Swap: `korek-ir` `include!`s a checked-in generated copy (`crates/korek-ir/src/generated.rs`), keeping
   `impls.rs`; a test regenerates it and fails on a diff (like the goldens, blessed with `KOREK_BLESS=1`).
4. The checked-in copy is the bootstrap: korek builds from it, never from its own output at build time, so a
   broken generator cannot break its own build; regenerate and review the diff when the schema changes.
