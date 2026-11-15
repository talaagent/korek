# Benchmarks

```sh
cargo bench -p korek-cli
```

[benches/gen.rs](crates/korek-cli/benches/gen.rs) runs on a synthetic schema
([synthetic.rs](crates/korek-testkit/src/synthetic.rs)): 2000 types (400
groups of an enum, two structs with 12 and 3 heavily validated fields, a
tagged union and an untagged one; tagged unions chain 20 deep), 500 keys in
50 blocks, 20 services with 100 RPCs; 660 KB of KDL in 18 files, or the same
schema as one file. `full_gen` runs the rust, proto, zod, ts and jsonschema
generators with their outputs already up to date.

Release build on a 32-core x86_64 Linux host that was shared with other
builds (load average 40–60), so expect ±20%; fastest / median of 30 samples
(20 for `full_gen`):

| Benchmark | Fastest | Median |
|---|---:|---:|
| `load_and_validate`, 18 files | 20.6 ms | 22.8 ms |
| `load_and_validate`, one file | 38.7 ms | 44.5 ms |
| `full_gen`, builtin plugins | 55.2 ms | 62.3 ms |
| `full_gen`, subprocess plugins | 88.1 ms | 98.3 ms |
| `full_gen`, cache warm | 2.75 ms | 2.89 ms |

The `encode_request` and `generator` benches (medians from an earlier run on
the same host; the generators have not changed since):

| Benchmark | Median |
|---|---:|
| encode `GenerateRequest` (JSON) | 2.5 ms |
| generator `rust` | 12.3 ms |
| generator `proto` | 6.5 ms |
| generator `zod` | 2.8 ms |
| generator `ts` | 3.5 ms |
| generator `jsonschema` | 13.3 ms (noisy; fastest 8.9 ms) |

## What the numbers rest on

- **Files are parsed in parallel**, one thread per file, with
  [kdl-kit](https://github.com/talaagent/kdl-kit)'s `kdl-tree` parser;
  lowering stays in file order. A one-file schema loads on one thread, so
  splitting a large schema into files helps.
- **Type lookups are hashed** (`korek_ir::TypeIndex`, built once per pass) in
  validation, default/example checking and the generators.
- **Regexes are compiled once** per pattern at load time (defaults, examples
  and pattern validation), not per check.
- Generator output buffers are sized from the schema up front.
- **Input-hash cache** (`--cache-dir`, default `target/korek-cache` when
  `./target` exists): blake3 over the schema files, lockfile, extensions,
  the executable and plugin binaries (path, size, mtime), options and output
  directory. A plugin whose key matches and whose outputs are byte-identical
  on disk is skipped; when all are, the schema is not loaded.
  `crates/korek-cli/tests/cli.rs` (`cache_skips_unchanged_runs`) tests the
  hits and misses.
- Generated Rust `validate()` does not allocate on valid values (a
  counting-allocator test, `crates/korek-fixture/tests/alloc.rs`); regexes
  are compiled once per call site (`OnceLock`). Generated Zod schemas are
  module-level constants.

The IR stays plain serde data (no interned names or index-based type
references): with hashed lookups the generators take 3–13 ms for 2000 types.
