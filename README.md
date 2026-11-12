# korek

*korek* is Taglish for "correct": define your types once, get them correct
everywhere.

You declare types (enums, structs, tagged and untagged unions), typed keys
(named settings with defaults, docs and validation) and RPC services in
[KDL v2](https://kdl.dev) files. korek validates them and generates:

| Plugin | Output |
|---|---|
| `korek-gen-rust` | serde types, `validate()` per type, `Default`s, typed key consts (`keys::SESSION_STEER_ENABLED: Key<bool>`) |
| `korek-gen-proto` | Protobuf with [protovalidate](https://protovalidate.com) rules for every validation rule, `service`s for ConnectRPC/gRPC, a `buf.yaml` |
| `korek-gen-zod` | Zod v4 schemas, each `satisfies z.ZodType<T>` against the TS types (never `z.infer`), and a typed `keys` record |
| `korek-gen-ts` | bare TypeScript types and a `Keys` interface; rules as JSDoc tags; no runtime dependency |
| `korek-gen-jsonschema` | JSON Schema 2020-12 (one file, or one per type for MCP tool inputs) |
| `korek-gen-docs` | markdown reference: types, keys tables, rules in words, units humanized; extra columns from extension metadata (`columns=Layers=ext:setting.props.layers`) |

Generators are plugins, like protoc's `protoc-gen-*`: the `korek` driver
loads and validates the schema, then sends each plugin the IR as a JSON
`GenerateRequest`. The CLI links the six first-party generators in-process
(the default `builtin` feature); a `korek-gen-<name>` on `PATH` or
`--plugin name=path` overrides them, and anything else runs as a subprocess.
A third-party plugin is about 20 lines ([example](examples/plugin-example/src/main.rs)).

## Usage

```sh
cargo install --git https://github.com/talaagent/korek korek-cli   # or: nix run github:talaagent/korek --
korek gen --rust src/gen --proto proto --zod web/gen/zod --ts web/gen/ts --jsonschema schemas schema/
```

```text
korek gen [--rust DIR] [--proto DIR] [--zod DIR] [--ts DIR] [--jsonschema DIR] [--docs DIR] [--out NAME=DIR]...
          [--<name>_opt k=v,..] [--opt NAME:k=v,..]... [--plugin NAME=PATH]... [--ext NAME]...
          [--lock FILE] [--check] [--strict] [--no-builtin] [--cache-dir DIR] [--no-cache]
          [--message-format human|json] SCHEMA_DIR
korek check SCHEMA_DIR   # validate only
korek ir SCHEMA_DIR      # the IR plugins receive, as JSON
```

Every `.kdl` file under `SCHEMA_DIR` is one schema. `--check` writes nothing
and fails if any output or the lockfile is stale (run it in CI). Only files
whose content changed are written, so mtimes are kept. Runs are cached
under `--cache-dir` (default `target/korek-cache` when `./target` exists):
a plugin whose inputs (schema files, lockfile, plugin binary, options,
output directory) hash as before and whose outputs are unchanged on disk
is skipped, and when every plugin is, the schema is not loaded at all. `--strict` turns
`korek::rule_unsupported` warnings into errors. `--message-format json`
prints diagnostics as JSON lines (code, severity, message, help, url,
labels with file/line/col). Every code is documented in
[docs/errors.md](docs/errors.md).

Options: `--rust_opt file=…`; `--proto_opt protovalidate=false,buf_yaml=false`;
`--zod_opt types=inline` (declare the TS types in the Zod file) or
`ts_path=../ts/types` (where `types=import`, the default, imports them from);
`--jsonschema_opt per_type=true`.

## The schema

From [examples/basic/schema](examples/basic/schema):

```kdl
package "example.v1"

enum Backend {
  doc "Which provider runs a turn."
  variant claude { doc "Claude Code." }
  variant codex { doc "OpenAI Codex." }
}

struct McpHttp {
  doc "An MCP server reached over HTTP."
  field url "string" {
    validate { url; prefix "https://" message="MCP servers must use HTTPS" }
    example "https://mcp.example.com/sse"
  }
  field timeout_ms "u32" default=30000 {
    unit "milliseconds"
    validate { gte 1; lte 600000 }
  }
}

union McpServer tag="type" {      // tagged: variants are structs
  validate { required }
  variant stdio "McpStdio"
  variant http "McpHttp"
}

union Limit {                     // untagged: the first variant that matches
  variant count "u64"
  variant preset "string"
}

keys "session" {
  doc "Per-session settings."
  key "steer.enabled" "bool" default=#false { doc "Give a new message to the running turn."; label "Steering"; widget "toggle" }
  key "timeout" "duration" default="600s" { validate { gte "1s"; lte "86400s" } }
  key "max_turns" "u32" default=20 { deprecated "use turn_limit"; alias "turns.max" }
}

keys "claude" {
  key "mcp_servers" "map<string,McpServer>" default="{}"
  key "api_key" "string" { secret; validate { min_len 20 }; meta "acme.ui" widget="password" }
}

service ContactsService {
  rpc Search input="SearchRequest" output="SearchResponse" idempotency="no_side_effects"
}
```

- **Types**: `bool`, `string`, `bytes`, `i32`, `i64`, `u32`, `u64`, `f32`,
  `f64`, `timestamp`, `duration`, `list<T>`, `map<string,T>`, named types.
  Fields can be `optional=#true` and have a `default=`. Recursive types are
  not supported yet.
- **Validation**: a `validate { … }` block; see [VALIDATION.md](VALIDATION.md)
  for every rule and what each target supports.
- **Docs and metadata** on every item: `doc` (one line), `description`
  (markdown, `"""` multiline), `example` (repeatable, checked against the
  type and rules), `unit`, `deprecated`, `alias`, `since`, `secret`, UI hints
  (`label`, `section`, `level basic|advanced`, `widget`), and namespaced
  `meta "<ns>" k=v` passed through untouched.
- **Defaults and examples**: native values for scalars and enums; a JSON
  string for lists, maps, structs and tagged unions (`default="{}"`).
- **Keys** are identified by their full dotted name: Rust
  `keys::SESSION_STEER_ENABLED`, TS/Zod `"session.steer.enabled"`.
- **Names**: types and services `PascalCase`; fields, variants and key
  segments `lower_snake_case`.

Errors point at the source, all at once, sorted by file and line:

```text
korek::unknown_type (https://github.com/talaagent/korek/blob/main/docs/errors.md#unknown_type)

  × unknown type `McpStdo`
    ╭─[types.kdl:24:17]
 24 │   variant stdio "McpStdo"
    ·                 ────┬────
    ·                     ╰── not declared
    ╰────
  help: did you mean `McpStdio`?
```

## Stable Protobuf numbers

Field, variant and key numbers are pinned by `id=` or recorded in
`korek.lock` next to the schema (commit it). New items get the next unused
number, removed ones become `reserved`, and an `id=` that disagrees with the
lockfile is an error.

## Output excerpts

Rust ([full](examples/basic/gen/rust/types.rs); needs `serde` and
`korek-runtime`):

```rust
pub struct McpHttp {
    pub url: String,
    /// Unit: milliseconds.
    #[serde(default = "McpHttp::default_timeout_ms")]
    pub timeout_ms: u32,
}

impl ::korek_runtime::Validate for McpHttp {
    fn validate_at(&self, path: &::korek_runtime::Path<'_>, errors: &mut ::korek_runtime::ValidationErrors) {
        {
            let path = path.field("url");
            let v = &self.url;
            if !::korek_runtime::format::url(v) {
                errors.push(&path, "url", "must be a valid url");
            }
            if !v.starts_with("https://") {
                errors.push(&path, "prefix", "MCP servers must use HTTPS");
            }
        }
        // …
    }
}
```

Protobuf ([full](examples/basic/gen/proto/example/v1/types.proto)):

```proto
message McpHttp {
  string url = 1 [(buf.validate.field) = {string: {uri: true, prefix: "https://"}}];
  // Unit: milliseconds.
  // Default: 30000
  uint32 timeout_ms = 2 [(buf.validate.field) = {uint32: {gte: 1, lte: 600000}}];
}

message McpServer {
  oneof value {
    option (buf.validate.oneof).required = true;
    McpStdio stdio = 1;
    McpHttp http = 2;
  }
}
```

Zod ([full](examples/basic/gen/zod/zod.ts)):

```ts
import type { McpHttp, McpServer, Keys /* … */ } from "../ts/types";

export const McpHttpSchema = z.object({
  /** @example "https://mcp.example.com/sse" */
  url: z.url().startsWith("https://", { error: "MCP servers must use HTTPS" }),
  /** Unit: milliseconds. */
  timeout_ms: z.uint32().gte(1).lte(600000).default(30000),
}) satisfies z.ZodType<McpHttp>;

export const keys: { [K in keyof Keys]-?: Key<Keys[K]> } = { /* "session.timeout": { schema, default, doc }, … */ };
```

TypeScript ([full](examples/basic/gen/ts/types.ts)) and JSON Schema
([full](examples/basic/gen/jsonschema/schema.json)) are alongside.

### ConnectRPC and protovalidate

`korek-gen-proto` writes a `buf.yaml` with the protovalidate dependency
(run `buf dep update` once for `buf.lock`). Stock buf plugins generate
clients and servers, e.g. this `buf.gen.yaml`:

```yaml
version: v2
inputs:
  - directory: proto
plugins:
  - remote: buf.build/bufbuild/es # messages and Connect services (connect-es v2)
    out: web/src/gen
    opt: target=ts
  - remote: buf.build/protocolbuffers/go
    out: gen/go
    opt: paths=source_relative
  - remote: buf.build/connectrpc/go
    out: gen/go
    opt: paths=source_relative
```

Validate at runtime with `@bufbuild/protovalidate` or
`buf.build/go/protovalidate`.

## As a library

Depend on korek from git (it is not published):

```toml
[dependencies]
korek = { git = "https://github.com/talaagent/korek" }
```

| Crate | What |
|---|---|
| `korek` | the facade: re-exports, generators (feature `generators`), and `Driver` |
| `korek-ir` | the IR and plugin protocol: pure serde data, minimal deps |
| `korek-plugin` | `run`, encoding, `Hook` (no parser dependency, so plugins build fast) |
| `korek-core` | parsing, lowering, validation, lockfile, extensions (`miette` feature for rendering) |
| `korek-gen-*` | one library per generator: `generate(&schema, &opts, hooks)` and `plugin(&req)` |
| `korek-runtime` | what generated Rust needs: `Validate`, `Key<T>`, formats, base64 |
| `korek-testkit` | shared validation vectors and a conformance runner for any generator |
| `korek-cli` | the `korek` binary and the `korek-gen-*` plugin binaries |

`korek_core::load(dir)` returns the IR; `korek_core::Loader` adds
extensions, a lockfile path and in-memory sources. Generators take
`korek_plugin::Hook`s, which add text after any type, key or service, or
extra files, without forking the generator.

### Extensions

Domain nodes and properties are rejected unless a `korek_core::Extension`
claims them. An extension turns them into JSON metadata on the item (or the
schema), reads it back typed with `ext_as::<T>()`, and can cross-check the
finished schema; the metadata reaches every plugin. An external crate can
register a node kind, validate it, and ship its own `korek-gen-*` plugin that
reads the metadata. See
[crates/korek-cli/tests/extension.rs](crates/korek-cli/tests/extension.rs)
for a `feature_flag` extension (`owner`, `expires`, keys `gated_by` a flag)
with a generator hook. From the CLI, `--ext NAME` captures nodes and
properties called `NAME` as generic JSON.

### Writing a plugin

```rust
use korek_plugin::{File, GenerateResponse};

fn main() -> std::process::ExitCode {
    korek_plugin::run(|req| {
        let names: Vec<_> = req.schema.types.iter().map(|t| t.name.as_str()).collect();
        Ok(GenerateResponse::new(vec![File::new("types.txt", names.join("\n"))]))
    })
}
```

Name it `korek-gen-<name>` and run `korek gen --out <name>=DIR`. Run its
output through `korek-testkit` to check it agrees with the shared vectors.

## Roadmap

Not in v1: `korek fmt`, a docs/markdown generator, schema imports across
packages, breaking-change detection against a git ref, an LSP, Python and Go
generators, recursive types, defining korek's own IR in a korek schema
(self-hosting; [schema/korek](schema/korek) is the start).

## Development

```sh
nix develop            # Rust (rust-toolchain.toml), bun, buf, protoc
cargo test --workspace
bun install && bun test && bun run typecheck
KOREK_BLESS=1 cargo test -p korek-cli --test golden    # regenerate goldens
KOREK_BLESS=1 cargo test -p korek-testkit               # after editing the vectors
cargo bench -p korek-cli                                # see BENCHMARKS.md
```

Licensed under Apache-2.0.
