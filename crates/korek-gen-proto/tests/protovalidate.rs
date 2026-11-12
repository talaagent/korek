//! Golden test of the protovalidate rules and the shapes Protobuf needs
//! help with (wrappers, oneofs, reserved numbers, services).

#![allow(clippy::needless_raw_string_hashes)]

use korek_core::{Loader, SourceFile};
use korek_gen_proto::{Options, generate};

const SCHEMA: &str = r#"
package "acme.v1"
enum Color { variant red; variant green }
struct Item {
  validate { cel id="qty_vs_ratio" expression="this.qty > 0 || !has(this.ratio)" }
  field name "string" { validate { min_len 1; max_len 64; pattern "^[a-z]+$"; not_in "root" "admin" } }
  field email "string" optional=#true { validate { email } }
  field code "string" optional=#true { validate { cuid2 } }
  field qty "i64" { validate { gte -5; lte 5; multiple_of 1 } }
  field ratio "f64" optional=#true { validate { gt 0.5; finite } }
  field blob "bytes" { validate { max_len 16 } }
  field tags "list<string>" { validate { min_items 1; max_items 3; unique; items { min_len 1 } } }
  field attrs "map<string,i64>" { validate { max_pairs 8; keys { pattern "^[a-z]+$" }; values { gte 0 } } }
  field grid "list<list<i64>>"
  field color "Color" { validate { defined_only; not_in "green" } }
  field choice "Choice"
  field maybe "Choice" optional=#true
  field at "timestamp" { validate { lt_now } }
  field ttl "duration" optional=#true { validate { lte "3600s" } }
}
union Choice { variant n "i64"; variant many "list<string>" }
service ItemService {
  rpc Put input="PutRequest" output="PutResponse" idempotency="idempotent"
}
struct PutRequest { field item "Item" }
struct PutResponse {}
"#;

fn proto(src: &str, lock: Option<String>) -> (String, String) {
    let loaded = Loader::new()
        .load_sources(
            vec![SourceFile {
                path: "s.kdl".into(),
                contents: src.into(),
            }],
            lock.map(|contents| SourceFile {
                path: "korek.lock".into(),
                contents,
            }),
        )
        .expect("the schema loads");
    let (files, warnings) = generate(&loaded.schema, &Options::default(), &[]);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(files[0].path, "acme/v1/types.proto");
    assert_eq!(files[1].path, "buf.yaml");
    (files[0].content.clone(), loaded.lock.render())
}

#[test]
fn rules_wrappers_and_services() {
    let (text, _) = proto(SCHEMA, None);
    for want in [
        "import \"buf/validate/validate.proto\";",
        "import \"google/protobuf/duration.proto\";",
        "import \"google/protobuf/timestamp.proto\";",
        "option (buf.validate.message).cel = {\n    id: \"qty_vs_ratio\"",
        r#"string name = 1 [(buf.validate.field) = {string: {min_len: 1, max_len: 64, pattern: "^[a-z]+$", not_in: ["root", "admin"]}}];"#,
        r#"optional string email = 2 [(buf.validate.field) = {string: {email: true}}];"#,
        r#"cel: [{id: "format.cuid2", message: "must be a valid cuid2", expression: "this.matches('^[0-9a-z]+$')"}]"#,
        r#"int64 qty = 4 [(buf.validate.field) = {int64: {gte: -5, lte: 5}, cel: [{id: "multiple_of""#,
        r#"optional double ratio = 5 [(buf.validate.field) = {double: {gt: 0.5, finite: true}}];"#,
        r#"bytes blob = 6 [(buf.validate.field) = {bytes: {max_len: 16}}];"#,
        r#"repeated string tags = 7 [(buf.validate.field) = {repeated: {min_items: 1, max_items: 3, unique: true, items: {string: {min_len: 1}}}}];"#,
        r#"map<string, int64> attrs = 8 [(buf.validate.field) = {map: {max_pairs: 8, keys: {string: {pattern: "^[a-z]+$"}}, values: {int64: {gte: 0}}}}];"#,
        "repeated Int64List grid = 9;",
        r#"Color color = 10 [(buf.validate.field) = {enum: {defined_only: true, not_in: [2]}}];"#,
        r#"Choice choice = 11 [(buf.validate.field) = {required: true}];"#,
        "Choice maybe = 12;",
        r#"google.protobuf.Timestamp at = 13 [(buf.validate.field) = {timestamp: {lt_now: true}, required: true}];"#,
        r#"google.protobuf.Duration ttl = 14 [(buf.validate.field) = {duration: {lte: {seconds: 3600}}}];"#,
        "oneof value {\n    option (buf.validate.oneof).required = true;\n    int64 n = 1;\n    StringList many = 2;",
        "message Int64List {\n  repeated int64 values = 1;\n}",
        "enum Color {\n  COLOR_UNSPECIFIED = 0;\n  COLOR_RED = 1;\n  COLOR_GREEN = 2;\n}",
        "rpc Put(PutRequest) returns (PutResponse) {\n    option idempotency_level = IDEMPOTENT;\n  }",
    ] {
        assert!(text.contains(want), "missing:\n{want}\n\nin:\n{text}");
    }
}

#[test]
fn removed_fields_are_reserved() {
    let (_, lock) = proto(
        "package \"acme.v1\"\nstruct A { field x \"string\"; field y \"string\" }",
        None,
    );
    let (text, _) = proto(
        "package \"acme.v1\"\nstruct A { field y \"string\"; field z \"string\" }",
        Some(lock),
    );
    assert!(
        text.contains("  string y = 2;\n  string z = 3;\n  reserved 1;\n  reserved \"x\";\n"),
        "{text}"
    );
}
