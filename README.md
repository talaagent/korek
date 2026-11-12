# korek

*korek* is Taglish for "correct": define your types once, and get them right
in every language.

You declare types (enums, structs, tagged and untagged unions), typed keys
(named settings with defaults, docs and constraints) and RPC services in
[KDL v2](https://kdl.dev) files. korek validates them and generates:

| Plugin | Output |
|---|---|
| `korek-gen-rust` | serde types, and a module of typed key consts (`Key<bool>`) per keys block, with default, doc and constraint metadata |
| `korek-gen-proto` | Protobuf messages, enums, `oneof`s, `map`/`repeated`/`optional`, [protovalidate](https://protovalidate.com) rules for every constraint, `service`s for ConnectRPC/gRPC, and a `buf.yaml` |
| `korek-gen-zod` | a Zod (v4) schema and inferred type per type, and a `keys` object of `{ schema, default, doc }` |
| `korek-gen-ts` | bare TypeScript `type`/`interface` declarations and string-literal unions, no runtime dependency |

Generators are plugins, like protoc's `protoc-gen-*`: the `korek` driver
parses and validates the schema, then sends each plugin the IR over a small
JSON protocol. Write your own in about 20 lines (see
[examples/plugin-example](examples/plugin-example/src/main.rs)).

## Usage

```sh
nix run github:talaagent/korek -- gen --rust src/gen --proto proto --zod web/gen --ts web/gen schema/
# or
cargo install --git https://github.com/talaagent/korek korek
```

```text
korek gen [--rust DIR] [--proto DIR] [--zod DIR] [--ts DIR] [--out NAME=DIR]...
          [--rust_opt k=v,..] [--proto_opt ..] [--zod_opt ..] [--ts_opt ..] [--opt NAME:k=v,..]...
          [--plugin NAME=PATH]... [--ext NAME]... [--lock FILE] [--check] SCHEMA_DIR
korek check SCHEMA_DIR     # validate only
korek ir SCHEMA_DIR        # print the IR plugins receive, as JSON
```

Every `.kdl` file under `SCHEMA_DIR` is part of one schema. `--check` writes
nothing and exits non-zero if any generated file (or the lockfile) is out of
date; run it in CI. `--out NAME=DIR` runs any `korek-gen-NAME` on `PATH`.

## The schema

From [examples/basic/schema](examples/basic/schema):

```kdl
package "example.v1"

enum Backend {
  doc "Which provider runs a turn."
  variant claude { doc "Claude Code." }
  variant codex { doc "OpenAI Codex." }
}

struct McpStdio {
  doc "An MCP server started as a child process."
  field command "string" { doc "The program to run."; min_len 1 }
  field args "list<string>" optional=#true
  field env "map<string,string>" optional=#true { doc "Extra environment variables." }
}

struct McpHttp {
  field url "string" { pattern "^https?://" }
  field timeout_ms "u64" optional=#true { min 1; max 600000 }
}

union McpServer tag="type" {      // tagged: variants are structs
  variant stdio "McpStdio"
  variant http "McpHttp"
}

union Limit {                     // untagged: the first variant that matches
  variant count "u64"
  variant preset "string"
}

keys "session" {
  doc "Per-session settings."
  key "steer.enabled" "bool" default=#false { doc "Give a new message to the running turn." }
  key "backend" "Backend" default="claude"
  key "timeout_s" "u64" default=600 { doc "Turn timeout in seconds."; min 1; max 86400 }
  key "turn_limit" "Limit" default=50
}

keys "claude" {
  key "mcp_servers" "map<string,McpServer>" default="{}"
}

service ContactsService {
  rpc Search input="SearchRequest" output="SearchResponse" idempotency="no_side_effects" {
    doc "Full-text search over names and emails."
  }
}
```

- **Types**: `bool`, `string`, `i64`, `u64`, `f64`, `bytes` (JSON: an array
  of byte values), `list<T>`, `map<string,T>`, and named types. Fields can be
  `optional=#true`. Recursive types are rejected in v1.
- **Constraints** (on fields and keys, as children or properties): `min`/`max`
  (numbers), `min_len`/`max_len` (characters, bytes, items or entries),
  `pattern` (a regex the string must match somewhere; anchor it with `^…$`).
- **Docs**: a `doc "…"` child (or `doc=` property) on every node.
- **Defaults**: native values for scalars and enums (`default=600`,
  `default="claude"`); a JSON string for lists, maps, bytes, structs and
  tagged unions (`default="{}"`). Defaults are checked against the type and
  every constraint, including those of nested struct fields.
- **Names**: types and services `PascalCase`, fields and variants
  `lower_snake_case`, keys and keys blocks dotted `lower_snake_case`.

Errors point at the source with miette:

```text
korek::unknown_type

  × unknown type `McpStdo`
    ╭─[types.kdl:24:17]
 23 │   doc "How to reach an MCP server."
 24 │   variant stdio "McpStdo"
    ·                 ────┬────
    ·                     ╰── not declared
 25 │   variant http "McpHttp"
    ╰────
  help: did you mean `McpStdio`?
```

## Stable Protobuf numbers

Field, variant and key numbers are pinned by an explicit `id=` or recorded in
`korek.lock` next to the schema (commit it). A new item gets the next unused
number, a removed one is `reserved` and never reused, and an `id=` that
disagrees with the lockfile is an error: numbers only change when you edit
the lockfile on purpose.

## Output excerpts

Rust ([full](examples/basic/gen/rust/types.rs)):

```rust
#[derive(Debug, Clone, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpServer {
    Stdio(McpStdio),
    Http(McpHttp),
}

/// Per-session settings.
pub mod session {
    /// Turn timeout in seconds.
    pub const TIMEOUT_S: super::Key<u64> = super::Key {
        group: "session",
        name: "timeout_s",
        doc: "Turn timeout in seconds.",
        default: Some(|| 600),
        default_json: Some("600"),
        constraints: &[super::Constraint::Min(1.0), super::Constraint::Max(86400.0)],
    };
}
```

Protobuf ([full](examples/basic/gen/proto/example/v1/types.proto)):

```proto
message McpHttp {
  string url = 1 [
    (buf.validate.field).required = true,
    (buf.validate.field).string.pattern = "^https?://"
  ];
  optional uint64 timeout_ms = 2 [
    (buf.validate.field).uint64.gte = 1,
    (buf.validate.field).uint64.lte = 600000
  ];
}

message McpServer {
  oneof value {
    option (buf.validate.oneof).required = true;
    McpStdio stdio = 1;
    McpHttp http = 2;
  }
}

service ContactsService {
  // Full-text search over names and emails.
  rpc Search(SearchRequest) returns (SearchResponse) {
    option idempotency_level = NO_SIDE_EFFECTS;
  }
}
```

Zod ([full](examples/basic/gen/zod/zod.ts)):

```ts
export const McpServerSchema = z.discriminatedUnion("type", [
  McpStdioSchema.extend({ type: z.literal("stdio") }),
  McpHttpSchema.extend({ type: z.literal("http") }),
]);
export type McpServer = z.infer<typeof McpServerSchema>;

export const keys = {
  session: {
    /** Turn timeout in seconds. */
    "timeout_s": key({
      schema: z.number().int().nonnegative().min(1).max(86400),
      default: 600,
      doc: "Turn timeout in seconds.",
    }),
  },
};
```

TypeScript ([full](examples/basic/gen/ts/types.ts)):

```ts
export type Backend = "claude" | "codex";
export type McpServer =
  | ({ type: "stdio" } & McpStdio)
  | ({ type: "http" } & McpHttp);
export interface SessionKeys {
  "steer.enabled": boolean;
  backend: Backend;
  timeout_s: number;
}
```

### ConnectRPC and protovalidate

`korek-gen-proto` writes a `buf.yaml` with the protovalidate dependency next
to the `.proto` files (run `buf dep update` once to create `buf.lock`). Stock
buf plugins then generate clients and servers, e.g. a `buf.gen.yaml` for
connect-es (TypeScript) and connect-go:

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

Validate at runtime with `@bufbuild/protovalidate` (TypeScript) or
`buf.build/go/protovalidate` (Go). Options: `--proto_opt protovalidate=false`
leaves the rules out, `buf_yaml=false` skips `buf.yaml`.

## As a library

korek is meant to be a dependency (as a git dependency; it is not published):

```toml
[dependencies]
korek = { git = "https://github.com/talaagent/korek" }
```

- `korek_core::load(dir)` returns the validated IR
  ([`korek_core::ir`](crates/korek-core/src/ir.rs)); `korek_core::Loader`
  adds extensions and a lockfile path, and `load_sources` loads in-memory
  files.
- Each generator crate has `generate(&schema, &Options, &hooks) -> Vec<File>`,
  callable in-process; a `korek_plugin::Hook` adds text after any type, keys
  block, key or service, or extra files, without forking the generator.
- `korek::Driver` is what the CLI runs: load, run plugins over the protocol,
  write or `check`.

### Extensions

Domain-specific nodes and properties are rejected unless an
`korek_core::Extension` claims them. An extension turns them into JSON
metadata stored in the IR's `ext` maps (on the schema for top-level nodes,
on the item otherwise), reads it back typed with `ext_as::<T>()`, and can
cross-check the finished schema. The metadata travels to every plugin, so a
third-party plugin can read it without changes to korek. From the CLI,
`--ext NAME` captures nodes and properties called `NAME` as generic JSON. See
[crates/korek/tests/extension.rs](crates/korek/tests/extension.rs) for a
`setting`/`layer` extension with a generator hook.

### Writing a plugin

A plugin reads a `GenerateRequest` (protocol version, korek version, its
options, the IR) on stdin and writes a `GenerateResponse` (files, plus errors
and warnings with optional spans into the schema) on stdout:

```rust
use korek_plugin::{File, GenerateResponse};

fn main() -> std::process::ExitCode {
    korek_plugin::run(|req| {
        let names: Vec<_> = req.schema.types.iter().map(|t| t.name.as_str()).collect();
        Ok(GenerateResponse::new(vec![File::new("types.txt", names.join("\n"))]))
    })
}
```

Name the binary `korek-gen-<name>` and run `korek gen --out <name>=DIR`, or
pass `--plugin <name>=PATH`. Run `korek ir` to see what a plugin receives.

## Development

```sh
nix develop            # Rust (rust-toolchain.toml), bun, buf, protoc
cargo test --workspace
bun install && bun test && bun run typecheck
KOREK_BLESS=1 cargo test -p korek --test golden   # regenerate examples/basic/gen
```

Licensed under Apache-2.0.
