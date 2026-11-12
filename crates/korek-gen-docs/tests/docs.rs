//! Options, metadata columns and hooks (the example schema's output is a
//! golden file in `examples/basic/gen/docs`).

#![allow(clippy::unwrap_used)]

extern crate alloc;

use alloc::collections::BTreeMap;
use core::fmt::Write as _;

use korek_core::{Loader, Passthrough, SourceFile};
use korek_gen_docs::{Column, ColumnSource, Options, generate, plugin};
use korek_ir::{Attrs, GenerateRequest, Schema, codes};
use korek_plugin::{Hook, Item};

const SCHEMA: &str = r#"
package "acme.v1"

struct Window {
  doc "A time window."
  field start "timestamp"
  field length "duration" { validate { gt "0s"; lte "3600s" } }
}

keys "ui" {
  doc "Display settings."
  key "theme" "string" default="light" {
    validate { in "light" "dark" }
    setting layers="user,project" apply="live"
    meta "acme.ui" widget="select"
  }
  key "max_upload" "u64" default=1048576 {
    unit "bytes"
    setting apply="restart"
  }
  key "token" "string" default="0123456789abcdef0123" {
    secret
    example "abc"
  }
}
"#;

fn schema() -> Schema {
    Loader::new()
        .extension(Passthrough::new("setting"))
        .load_sources(
            vec![SourceFile {
                path: "s.kdl".into(),
                contents: SCHEMA.into(),
            }],
            None,
        )
        .unwrap()
        .schema
}

fn one(opts: &Options) -> String {
    let files = generate(&schema(), opts, &[]);
    assert_eq!(files.len(), 1);
    files.into_iter().next().unwrap().content
}

#[test]
fn rules_units_and_secrets_read_as_words() {
    let md = one(&Options::default());
    assert!(md.contains("# acme.v1 reference"), "{md}");
    assert!(
        md.contains("Rules: Greater than `\"0s\"` (0 seconds); at most `\"3600s\"` (1 hour)."),
        "{md}"
    );
    assert!(md.contains("`1048576` (1 MiB)"), "{md}");
    assert!(
        md.contains("Rules: One of `\"light\"`, `\"dark\"`."),
        "{md}"
    );
    // A secret's default and examples are never shown.
    assert!(!md.contains("0123456789abcdef0123"), "{md}");
    assert!(!md.contains("\"abc\""), "{md}");
    assert!(md.contains("| *hidden* |"), "{md}");
}

#[test]
fn columns_come_from_metadata() {
    let mut opts = Options::default();
    opts.columns = vec![
        Column::parse("Layers=ext:setting.props.layers").unwrap(),
        Column::parse("Apply=ext:setting.props.apply").unwrap(),
        Column::parse("Widget=meta:acme.ui.widget").unwrap(),
        Column::parse("Unused=ext:nothing").unwrap(),
        Column::new(
            "Secret",
            ColumnSource::With(|a: &Attrs| a.secret.then(|| "yes".to_owned())),
        ),
    ];
    let md = one(&opts);
    assert!(
        md.contains("| Key | Type | Default | Description | Layers | Apply | Widget | Secret |"),
        "{md}"
    );
    assert!(md.contains("| user,project | live | select |  |"), "{md}");
    assert!(md.contains("|  | restart |  |  |"), "{md}");
    assert!(md.contains("- Apply: restart\n"), "{md}");
    assert!(!md.contains("Unused"), "{md}");
    // Struct tables have no metadata, so no extra columns.
    assert!(
        md.contains("| Field | Type | Default | Description |\n"),
        "{md}"
    );
}

#[test]
fn split_and_heading_level() {
    let mut opts = Options::default();
    opts.split = true;
    opts.heading_level = 2;
    let files = generate(&schema(), &opts, &[]);
    let names: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(names, ["index.md", "types.md", "keys.ui.md"]);
    assert!(files[0].content.contains("## acme.v1 reference"));
    assert!(files[0].content.contains("[`ui`](keys.ui.md#keys.ui)"));
    assert!(files[1].content.contains("### Window (struct)"));
    assert!(
        files[2]
            .content
            .contains("<a id=\"keys.ui\"></a>\n\n## `ui` keys")
    );
    assert!(files[2].content.contains("### `ui.theme`"));
}

struct Note;

impl Hook for Note {
    fn after(&self, item: Item<'_>, out: &mut String) {
        if let Item::Key(_, k) = item {
            let _ = write!(out, "> hooked {}\n\n", k.name);
        }
    }

    fn footer(&self, _: &Schema, out: &mut String) {
        out.push_str("Footer.\n");
    }
}

#[test]
fn hooks_add_text() {
    let files = generate(&schema(), &Options::default(), &[&Note]);
    let md = &files[0].content;
    assert!(md.contains("> hooked theme\n"), "{md}");
    assert!(md.ends_with("Footer.\n"), "{md}");
}

#[test]
fn plugin_options() {
    let req = |opts: &[(&str, &str)]| {
        GenerateRequest::new("docs", schema()).with_options(
            opts.iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect::<BTreeMap<_, _>>(),
        )
    };
    let resp = plugin(&req(&[
        ("file", "SETTINGS.md"),
        ("title", "Settings"),
        ("columns", "Apply=ext:setting.props.apply"),
    ]))
    .unwrap();
    assert_eq!(resp.files[0].path, "SETTINGS.md");
    assert!(resp.files[0].content.contains("# Settings\n"));
    assert!(resp.files[0].content.contains("| Apply |"));
    for bad in [
        ("heading_level", "9"),
        ("columns", "Apply=setting"),
        ("split", "maybe"),
        ("nope", "1"),
    ] {
        let err = plugin(&req(&[bad])).unwrap_err();
        assert_eq!(err.code, codes::INVALID_OPTION, "{bad:?}");
    }
}
