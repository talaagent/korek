# korek diagnostics

Every diagnostic korek emits has a stable code. Codes are API: `snake_case`, never reused.
Plugins namespace their own codes (`korek_gen_proto::…`). `korek gen --message-format json` prints them machine-readably, one JSON object per line:
`code`, `severity`, `message`, `help`, `url`, the primary location `file`, `line` and `col` (all `null`
for errors with no source location, like I/O and plugin failures) and `labels`.

## `korek::syntax`

Error. The file is not valid KDL v2.

```kdl
struct A {   // no closing brace
```

## `korek::io`

Error. A schema file or directory could not be read.

```kdl
korek check missing-dir/
```

## `korek::unknown_node`

Error. A node the grammar does not know and no extension claims.

```kdl
struct A { fild x "string" }
```

## `korek::unknown_property`

Error. A property the grammar does not know and no extension claims.

```kdl
struct A { field x "string" opt=#true }
```

## `korek::unknown_type`

Error. A type reference to an undeclared type.

```kdl
struct A { field b "Strng" }
```

## `korek::unknown_rule`

Error. A rule name inside `validate` that korek does not know.

```kdl
field x "string" { validate { emial } }
```

## `korek::duplicate_name`

Error. Two items with the same name in one scope.

```kdl
struct A {}
enum A { variant x }
```

## `korek::name_clash`

Error. A declared name clashes with one korek generates.

```kdl
struct SessionKeys {}
keys "session" {}
```

## `korek::invalid_name`

Error. A name that breaks the naming rules.

```kdl
struct lower_case {}
```

## `korek::invalid_type`

Error. A malformed type expression.

```kdl
field x "map<i64,string>"
```

## `korek::invalid_value`

Error. A malformed argument or property value.

```kdl
rpc Get input="A" output="B" idempotency="sometimes"
```

## `korek::missing_value`

Error. A required argument or property is missing.

```kdl
field x
```

## `korek::invalid_default`

Error. A default that does not match its type or rules.

```kdl
key "a" "u64" default=-1
```

## `korek::invalid_example`

Error. An example that does not match its type or rules.

```kdl
field x "string" { example 5 }
```

## `korek::invalid_rule`

Error. A rule that does not apply to its type or has a bad operand.

```kdl
field a "bool" { validate { gte 1 } }
```

## `korek::rule_unsupported`

Warning. A generator cannot express a rule.

```kdl
field x "string" { validate { emoji } }   // no protovalidate rule
```

## `korek::invalid_union`

Error. A tagged union with a non-struct variant or a clashing tag.

```kdl
union U tag="t" { variant a "string" }
```

## `korek::invalid_rpc`

Error. An rpc whose input or output is not a message.

```kdl
enum E { variant x }
service S { rpc Do input="E" output="E" }
```

## `korek::cycle`

Error. A recursive type (not supported yet).

```kdl
struct A { field b "B" }
struct B { field a "list<A>" }
```

## `korek::empty`

Error. An enum or union without variants.

```kdl
enum E {}
```

## `korek::invalid_id`

Error. An explicit id out of range or reserved.

```kdl
field x "string" id=0
```

## `korek::duplicate_id`

Error. Two items pinned to the same id.

```kdl
field x "string" id=2
field y "string" id=2
```

## `korek::id_taken`

Error. An id the lockfile gives to another item.

```kdl
field y "string" id=1   // korek.lock gives 1 to x
```

## `korek::renumbered`

Error. An id that disagrees with the lockfile.

```kdl
field x "string" id=7   // korek.lock records 1
```

## `korek::lockfile`

Error. A malformed korek.lock.

```kdl
struct "A" { item "x" }   // in korek.lock: missing number
```

## `korek::plugin_failed`

Error. A plugin exited unsuccessfully or reported errors.

```kdl
korek-gen-foo exited with status 1
```

## `korek::plugin_protocol`

Error. A plugin's response broke the protocol.

```kdl
korek-gen-foo wrote something that is not a GenerateResponse
```

## `korek::plugin_not_found`

Error. No binary for a requested plugin.

```kdl
korek gen --out foo=gen/ schema/   # no korek-gen-foo
```

## `korek::invalid_option`

Error. An unknown or malformed plugin option.

```kdl
korek gen --rust gen/ --rust_opt nope=1 schema/
```

## `korek::stale_output`

Error. `--check`: a generated file is out of date.

```kdl
korek gen --check ...   # after editing the schema
```
