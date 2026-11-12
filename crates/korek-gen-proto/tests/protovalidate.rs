//! Golden test of the protovalidate rules and the shapes Protobuf needs
//! help with (wrappers, oneofs, reserved numbers, services).

use korek_core::{Loader, SourceFile};
use korek_gen_proto::{Options, generate};

const SCHEMA: &str = r#"
package "acme.v1"
enum Color { variant red; variant green }
struct Item {
  field name "string" { min_len 1; max_len 64; pattern "^[a-z]+$" }
  field note "string" optional=#true
  field qty "i64" { min -5; max 5 }
  field ratio "f64" optional=#true { min 0.5 }
  field blob "bytes" { max_len 16 }
  field tags "list<string>" { min_len 1; max_len 3 }
  field attrs "map<string,i64>" { max_len 8 }
  field grid "list<list<i64>>"
  field color "Color"
  field choice "Choice"
  field maybe "Choice" optional=#true
}
union Choice { variant n "i64"; variant many "list<string>" }
service ItemService {
  rpc Put input="PutRequest" output="PutResponse" idempotency="idempotent"
}
struct PutRequest { field item "Item" }
struct PutResponse {}
"#;

const EXPECTED: &str = r#"message Item {
  string name = 1 [
    (buf.validate.field).required = true,
    (buf.validate.field).string.min_len = 1,
    (buf.validate.field).string.max_len = 64,
    (buf.validate.field).string.pattern = "^[a-z]+$"
  ];
  optional string note = 2;
  int64 qty = 3 [
    (buf.validate.field).int64.gte = -5,
    (buf.validate.field).int64.lte = 5
  ];
  optional double ratio = 4 [(buf.validate.field).double.gte = 0.5];
  bytes blob = 5 [(buf.validate.field).bytes.max_len = 16];
  repeated string tags = 6 [
    (buf.validate.field).repeated.min_items = 1,
    (buf.validate.field).repeated.max_items = 3
  ];
  map<string, int64> attrs = 7 [(buf.validate.field).map.max_pairs = 8];
  repeated Int64List grid = 8;
  Color color = 9 [(buf.validate.field).enum.defined_only = true];
  Choice choice = 10 [(buf.validate.field).required = true];
  Choice maybe = 11;
}

message Choice {
  oneof value {
    option (buf.validate.oneof).required = true;
    int64 n = 1;
    StringList many = 2;
  }
}
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
    let files = generate(&loaded.schema, &Options::default(), &[]);
    assert_eq!(files[0].path, "acme/v1/types.proto");
    assert_eq!(files[1].path, "buf.yaml");
    (files[0].content.clone(), loaded.lock.render())
}

#[test]
fn rules_wrappers_and_services() {
    let (text, _) = proto(SCHEMA, None);
    assert!(
        text.contains("import \"buf/validate/validate.proto\";"),
        "{text}"
    );
    assert!(text.contains(EXPECTED), "{text}");
    for wrapper in [
        "message Int64List {\n  repeated int64 values = 1;\n}",
        "message StringList {\n  repeated string values = 1;\n}",
    ] {
        assert!(text.contains(wrapper), "{text}");
    }
    assert!(text.contains(
        "enum Color {\n  COLOR_UNSPECIFIED = 0;\n  COLOR_RED = 1;\n  COLOR_GREEN = 2;\n}"
    ));
    assert!(text.contains(
        "service ItemService {\n  rpc Put(PutRequest) returns (PutResponse) {\n    option idempotency_level = IDEMPOTENT;\n  }\n}"
    ));
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
        text.contains("  string y = 2 [(buf.validate.field).required = true];\n  string z = 3 [(buf.validate.field).required = true];\n  reserved 1;\n  reserved \"x\";\n"),
        "{text}"
    );
}
