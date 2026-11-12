# Benchmarks

`cargo bench -p korek-cli` ([benches/gen.rs](crates/korek-cli/benches/gen.rs))
on a synthetic schema ([synthetic.rs](crates/korek-testkit/src/synthetic.rs)):
2000 types (400 groups of an enum, two structs with 12 and 3 heavily
validated fields, a tagged union and an untagged one; tagged unions chain 20
deep), 500 keys in 50 blocks, 20 services with 100 RPCs; 660 KB of KDL in 18
files. `full_gen` runs all five generators with outputs already up to date.

Medians, release build, 32-core x86_64 Linux shared with other builds (load
average 20–40 during the runs, so expect ±20%). "Before" is 0b6464e (the
code before the performance work) with the same benchmark.

| Benchmark | Before | After | |
|---|---:|---:|---|
| load + validate, 18 files | 265 ms | 55 ms | 4.8× |
| load + validate, one file | 254 ms | 232 ms | KDL parsing bound |
| encode `GenerateRequest` (JSON) | 2.6 ms | 2.5 ms | |
| generator `rust` | 14.2 ms | 12.3 ms | |
| generator `proto` | 8.9 ms | 6.5 ms | |
| generator `zod` | 7.8 ms | 2.8 ms | |
| generator `ts` | 5.5 ms | 3.5 ms | |
| generator `jsonschema` | 9.2 ms | 13.3 ms | noise (fastest 7.3 → 8.9 ms); unchanged code |
| `korek gen`, builtin plugins | 392 ms | 155 ms | 2.5× |
| `korek gen`, subprocess plugins | 396 ms | 196 ms | 2.0× |
| `korek gen`, cache warm | — | 3.0 ms | 130× vs builtin |

What changed:

- **KDL files are parsed in parallel** (one thread per file); lowering stays
  in file order. Parsing with the `kdl` crate is ~200 ms of the one-file
  load, so a single huge file gains little: split large schemas.
- **Type lookups are hashed** (`korek_ir::TypeIndex`, built once per pass)
  in validation, default/example checking and the generators; `Schema::get`
  was a linear scan, quadratic over a large schema.
- **Regexes are compiled once** per pattern at load time (defaults,
  examples and pattern validation), not per check.
- Defaults and examples are placed into the IR in one pass (was a scan per
  field).
- Generator output buffers are sized from the schema up front.
- **Input-hash cache** (`--cache-dir`, default `target/korek-cache` when
  `./target` exists): blake3 over the schema files, lockfile, extensions,
  the executable and plugin binaries (path, size, mtime), options and output
  directory. A plugin whose key matches and whose outputs are byte-identical
  on disk is skipped; when all are, the schema is not even loaded.
- Generated Rust `validate()` does not allocate on valid values (a
  counting-allocator test in `crates/korek-fixture/tests/alloc.rs`);
  regexes are compiled once per call site (`OnceLock`). Generated Zod
  schemas are module-level constants.

Not done: interning names (`Arc<str>`) and index-based type references in
the IR — with the lookups hashed, generators are 3–13 ms for 2000 types and
loading is dominated by KDL parsing, so the IR stays plain serde data.
Profiling with perf/samply was not possible on the benchmark host
(perf events are blocked); phases were timed with instrumentation instead
(KDL parse ~230 ms, lowering ~50 ms, validation ~25 ms, numbering ~3 ms,
before the changes on the one-file schema).

## After kdl-load (kdl-serde 8bda293)

Sources, listing and parsing moved to kdl-load/kdl-span/kdl-nodes, which
needs `kdl` 6.7 (was 6.5.0; one copy, now 6.7.1). The host was even busier
(load average 40–75), so each configuration was run twice, interleaved, on
`load_and_validate`; fastest / median of 30 samples:

| Configuration | 18 files | one file |
|---|---:|---:|
| before (fe19af7, kdl 6.5.0) | 65–82 / 137–167 ms | 239–247 / 312 ms |
| before, kdl 6.7.1 only | 67–90 / 143–165 ms | 338–341 / 431–445 ms |
| after (kdl-load, kdl 6.7.1) | 86–91 / 125–171 ms | 314–316 / 418–425 ms |

The kdl-load switch itself is neutral (same parallel per-file parsing,
spans unchanged). The one-file slowdown, ~35% fastest-to-fastest, comes
from the `kdl` 6.5 → 6.7 parser and also hits kdl-serde; the 18-file load
is within the noise. `full_gen` and the generators were not touched; their
runs here differed by up to 2× between identical builds.
