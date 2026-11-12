//! The driver runs this plugin over the protocol; extension metadata
//! reaches it without any change to korek.

use korek::{Driver, Output, Passthrough};

#[test]
fn driver_runs_the_example_plugin() {
    let tmp = tempfile::tempdir().unwrap();
    let schema = tmp.path().join("schema");
    std::fs::create_dir(&schema).unwrap();
    std::fs::write(
        schema.join("keys.kdl"),
        r#"keys "session" {
  key "timeout_s" "u64" default=600 {
    doc "Turn timeout."
    setting layers="user"
  }
}
"#,
    )
    .unwrap();
    let out = tmp.path().join("docs");
    let report = Driver::new()
        .extension(Passthrough::new("setting"))
        .plugin("example", env!("CARGO_BIN_EXE_korek-gen-example"))
        .output(Output::new("example", &out))
        .run(&schema)
        .unwrap();
    assert_eq!(report.files.len(), 2, "keys.md and the lockfile");
    let md = std::fs::read_to_string(out.join("keys.md")).unwrap();
    assert!(
        md.contains("| `session.timeout_s` | `u64` | `600` | Turn timeout. |"),
        "{md}"
    );
    assert!(
        md.contains(r#""setting":[{"name":"setting","props":{"layers":"user"}}]"#),
        "{md}"
    );
}
