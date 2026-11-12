# Validation

Rules live in a `validate { … }` block on a field, a key, a type (unions:
`required`; structs: `cel`), or on list items, map keys and map values
(`items { … }`, `keys { … }`, `values { … }`):

```kdl
field port "u32" { validate { gte 1; lte 65535 } }
field email "string" { validate { email; max_len 254 } }
field tags "list<string>" { validate { min_items 1; unique; items { min_len 1; pattern "^[a-z-]+$" } } }
field labels "map<string,string>" { validate { max_pairs 64; keys { max_len 63 }; values { max_len 256 } } }
union McpServer tag="type" { validate { required } … }
struct Range { validate { cel id="ordered" expression="this.lo <= this.hi" rust="v.lo <= v.hi" ts="(v) => v.lo <= v.hi" } … }
```

Every rule takes an optional `message="…"` (used by Rust, Zod and CEL rules;
protovalidate's standard rules keep their own messages). Defaults and
`example`s are checked against the rules when the schema loads.

A generator that cannot express a rule reports `korek::rule_unsupported` (a
warning; `korek gen --strict` makes it an error). The shared vectors in
`crates/korek-testkit/vectors/rules.json` run against the loader, the
generated Rust and the generated Zod; they must all agree.

## Coverage

✓ native · ≈ via a regex/CEL/refinement equivalent · — not expressible (warning) · n/a always true

| Rule | Applies to | Protobuf (protovalidate) | Zod | Rust `validate()` | JSON Schema |
|---|---|---|---|---|---|
| `const` | scalars, enums, timestamp | ✓ | ≈ refine | ✓ | ✓ `const` |
| `in` / `not_in` | scalars, enums, duration | ✓ | ≈ refine | ✓ | ✓ `enum` / `not` |
| `len` / `min_len` / `max_len` | string (characters), bytes | ✓ | ≈ refine (code points) | ✓ | ✓ strings; — bytes |
| `len_bytes` / `min_bytes` / `max_bytes` | string (UTF-8 bytes) | ✓ | ≈ refine | ✓ | — |
| `pattern` | string, bytes | ✓ (RE2) | ✓ `.regex` | ✓ (`regex` crate) | ✓ strings |
| `prefix` / `suffix` / `contains` | string, bytes | ✓ | ✓ / ≈ for bytes | ✓ | ≈ `pattern` |
| `not_contains` | string | ✓ | ≈ refine | ✓ | ≈ `not.pattern` |
| `email` `hostname` `ipv4` `ipv6` `uuid` `ulid` | string | ✓ | ✓ | ✓ | ✓ / ≈ |
| `ip` | string, bytes | ✓ | ≈ | ✓ | ≈ `anyOf` |
| `ip_with_prefixlen` `cidr` `host_and_port` `address` `uri_ref` | string | ✓ | — | ✓ | ✓ `uri-reference`; — others |
| `uri` / `url` | string | ✓ `uri` | ≈ / ✓ `z.url()` | ✓ | ✓ `uri` |
| `tuuid` | string | ✓ | ≈ regex | ✓ | ≈ |
| `cuid` `cuid2` `nanoid` `base64` `base64url` `jwt` `e164` `lowercase` `uppercase` | string | ≈ CEL `matches` | ✓ | ✓ | ≈ `pattern` |
| `datetime` `date` `time` `iso_duration` | string | ≈ CEL `matches` (calendar-aware dates) | ✓ `z.iso.*` | ✓ | ✓ `format` |
| `emoji` | string | — | ✓ | ✓ (`regex` feature) | — |
| `gt` / `gte` / `lt` / `lte` | numbers, timestamp, duration | ✓ | ✓ / ≈ for time | ✓ | ✓ numbers; — time |
| `finite` | floats | ✓ | n/a (Zod rejects ±∞) | ✓ | n/a |
| `multiple_of` | numbers | ≈ CEL (ints); — floats | ✓ | ✓ | ✓ |
| `min_items` / `max_items` / `unique` | list | ✓ | ✓ / ≈ | ✓ | ✓ |
| `min_pairs` / `max_pairs` | map | ✓ | ≈ refine | ✓ | ✓ |
| `items` / `keys` / `values` | list, map | ✓ | ✓ | ✓ | ✓ |
| `required` | struct/union fields, enums, unions | ✓ (`required`, `oneof.required`) | ✓ | ✓ | ✓ `required` |
| `defined_only` | enums | ✓ | n/a | n/a | n/a |
| `gt_now` / `lt_now` / `within` | timestamp | ✓ | ≈ refine | ✓ | — |
| `cel` | any | ✓ field/message CEL | with `ts="…"`, else — | with `rust="…"`, else — | — |

Non-optional message fields (structs, unions, timestamps, durations) are
always `required` in Protobuf: korek's "present" is protovalidate's
"required".

## JSON forms

All targets agree on one JSON form per type (the generated Rust serde, Zod,
TypeScript and JSON Schema): integers are numbers (also as Rust `Duration`s
under `rust_duration`); `bytes` is base64 at any depth (`list<bytes>`,
`map<string,bytes>`, …);
`timestamp` is RFC 3339; `duration` is seconds with an `s` suffix (`"1.5s"`,
as in the Protobuf JSON mapping); enums are their variant names; tagged
unions carry the tag field next to the variant's fields.
