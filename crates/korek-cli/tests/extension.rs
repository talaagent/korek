//! A neutral example extension: `feature_flag` nodes with an owner and an
//! expiry, referenced from keys, with typed metadata that travels to plugins
//! and a generator hook that adds output for it.

#![allow(clippy::unwrap_used)]

use core::fmt::Write as _;

use korek::ir::{Diagnostic, File, GenerateRequest, Schema, ext_as};
use korek::korek_core::ext::{Extension, ItemKind, Node};
use korek::korek_core::{Loader, SourceFile};
use korek::plugin::{Hook, Item};
use serde::{Deserialize, Serialize};

/// `feature_flag "new_ui" owner="team-ui" expires="2027-01-01"`.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Flag {
    name: String,
    owner: String,
    expires: String,
}

/// `gated_by "new_ui"` inside a key.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Gate {
    flag: String,
}

struct FeatureFlags;

fn string_prop(node: Node<'_>, name: &str) -> Result<String, Diagnostic> {
    node.prop(name)
        .and_then(|a| a.value.as_str().map(str::to_owned))
        .ok_or_else(|| {
            Diagnostic::error(
                "feature_flag::missing",
                format!("`{}` needs {name}=\"…\"", node.name()),
            )
            .at(node.span(), "here")
        })
}

impl Extension for FeatureFlags {
    fn name(&self) -> &'static str {
        "feature_flag"
    }

    fn claims_top_level(&self, node: &str) -> bool {
        node == "feature_flag"
    }

    fn claims_child(&self, kind: ItemKind, node: &str) -> bool {
        kind == ItemKind::Key && node == "gated_by"
    }

    fn top_level(&self, node: Node<'_>) -> Result<serde_json::Value, Diagnostic> {
        let name = node
            .args()
            .first()
            .and_then(|a| a.value.as_str().map(str::to_owned))
            .unwrap_or_default();
        let owner = string_prop(node, "owner")?;
        let expires = string_prop(node, "expires")?;
        if !korek::runtime::format::date(&expires) {
            let at = node.prop("expires").map_or(node.span(), |a| a.span);
            return Err(Diagnostic::error(
                "feature_flag::expires",
                "expires must be a date (YYYY-MM-DD)",
            )
            .at(at, "not a date"));
        }
        Ok(serde_json::to_value(Flag {
            name,
            owner,
            expires,
        })
        .unwrap())
    }

    fn child(&self, _: ItemKind, node: Node<'_>) -> Result<serde_json::Value, Diagnostic> {
        let flag = node
            .args()
            .first()
            .and_then(|a| a.value.as_str().map(str::to_owned))
            .unwrap_or_default();
        Ok(serde_json::to_value(Gate { flag }).unwrap())
    }

    fn check(&self, schema: &Schema) -> Vec<Diagnostic> {
        let flags: Vec<Flag> = schema.ext_as("feature_flag").unwrap();
        let mut out = Vec::new();
        for g in &schema.keys {
            for k in &g.keys {
                for gate in ext_as::<Gate>(&k.attrs.ext, "feature_flag").unwrap() {
                    if !flags.iter().any(|f| f.name == gate.flag) {
                        out.push(
                            Diagnostic::error(
                                "feature_flag::unknown",
                                format!("unknown feature flag `{}`", gate.flag),
                            )
                            .at(k.span, "in this key"),
                        );
                    }
                }
            }
        }
        out
    }
}

const SCHEMA: &str = r#"
feature_flag "new_ui" owner="team-ui" expires="2027-01-01"

keys "ui" {
  key "theme" "string" default="light" {
    gated_by "new_ui"
  }
  key "density" "u32" default=2
}
"#;

fn load(src: &str) -> Result<korek::korek_core::Loaded, korek::korek_core::Failure> {
    Loader::new().extension(FeatureFlags).load_sources(
        vec![SourceFile {
            path: "s.kdl".into(),
            contents: src.into(),
        }],
        None,
    )
}

#[test]
fn metadata_is_typed_and_travels_to_plugins() {
    let schema = load(SCHEMA).unwrap().schema;
    let key = &schema.keys[0].keys[0];
    assert_eq!(
        ext_as::<Gate>(&key.attrs.ext, "feature_flag").unwrap(),
        [Gate {
            flag: "new_ui".into()
        }]
    );
    let req = GenerateRequest::new("any", schema);
    let enc = korek::plugin::Encoding::Json;
    let back: GenerateRequest = enc.decode(&enc.encode(&req).unwrap()).unwrap();
    assert_eq!(back, req);
    let flags: Vec<Flag> = back.schema.ext_as("feature_flag").unwrap();
    assert_eq!(flags[0].owner, "team-ui");
}

#[test]
fn extension_errors_have_spans() {
    let f = load("feature_flag \"x\" owner=\"a\" expires=\"soon\"").unwrap_err();
    assert!(f.has("feature_flag::expires"), "{f}");
    assert!(f.diagnostics[0].span().is_some());
    let f = load("keys \"s\" { key \"a\" \"bool\" { gated_by \"nope\" } }").unwrap_err();
    assert!(f.has("feature_flag::unknown"), "{f}");
    let f = Loader::new()
        .load_sources(
            vec![SourceFile {
                path: "s.kdl".into(),
                contents: SCHEMA.into(),
            }],
            None,
        )
        .unwrap_err();
    assert!(f.has("korek::unknown_node"), "{f}");
}

/// Adds a `<KEY>_FLAG` const after each gated key, and a file.
struct FlagConsts;

impl Hook for FlagConsts {
    fn after(&self, item: Item<'_>, out: &mut String) {
        if let Item::Key(g, k) = item {
            for gate in ext_as::<Gate>(&k.attrs.ext, "feature_flag").unwrap() {
                let _ = writeln!(
                    out,
                    "    pub const {}_FLAG: &str = {:?};",
                    g.key_ident(k).to_uppercase(),
                    gate.flag
                );
            }
        }
    }

    fn files(&self, schema: &Schema) -> Vec<File> {
        let n = schema.ext_as::<Flag>("feature_flag").unwrap().len();
        vec![File::new("flags.txt", format!("{n} flags\n"))]
    }
}

#[test]
fn generator_hooks_add_output() {
    let schema = load(SCHEMA).unwrap().schema;
    let (files, _) = korek::generators::rust::generate(
        &schema,
        &korek::generators::rust::Options::default(),
        &[&FlagConsts],
    );
    assert_eq!(files.len(), 2);
    assert!(
        files[0]
            .content
            .contains("pub const UI_THEME_FLAG: &str = \"new_ui\";"),
        "{}",
        files[0].content
    );
    assert_eq!(files[1], File::new("flags.txt", "1 flags\n"));
}
