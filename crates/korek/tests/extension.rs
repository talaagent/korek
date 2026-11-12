//! An example extension: `layer` nodes and a `setting` block on keys, with
//! typed metadata that travels to plugins and a generator hook that adds
//! output for it.

use korek_core::diag::Diagnostic;
use korek_core::ext::{Extension, ItemKind, Literal, Node};
use korek_core::ir::{Schema, ext_as};
use korek_core::{Loader, SourceFile};
use korek_plugin::{File, GenerateRequest, Hook, Item};
use serde::{Deserialize, Serialize};

/// `layer "user" { doc "…" }` at the top level.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Layer {
    name: String,
}

/// `setting layers="user project" merge="replace"` inside a key.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Setting {
    layers: Vec<String>,
    merge: String,
}

struct Settings;

impl Extension for Settings {
    fn name(&self) -> &'static str {
        "settings"
    }

    fn claims_top_level(&self, node: &str) -> bool {
        node == "layer"
    }

    fn claims_child(&self, kind: ItemKind, node: &str) -> bool {
        kind == ItemKind::Key && node == "setting"
    }

    fn top_level(&self, node: Node<'_>) -> Result<serde_json::Value, Diagnostic> {
        let name = node
            .args()
            .first()
            .and_then(|a| a.value.as_str().map(str::to_owned))
            .ok_or_else(|| Diagnostic::error("settings::layer", "`layer` needs a name").at(node.span(), "here"))?;
        Ok(serde_json::to_value(Layer { name }).unwrap())
    }

    fn child(&self, _: ItemKind, node: Node<'_>) -> Result<serde_json::Value, Diagnostic> {
        let layers = node
            .prop("layers")
            .and_then(|a| a.value.as_str().map(|s| s.split_whitespace().map(str::to_owned).collect()))
            .unwrap_or_default();
        let merge = match node.prop("merge") {
            None => "replace".to_owned(),
            Some(a) => match &a.value {
                Literal::String(s) if s == "replace" || s == "append" => s.clone(),
                _ => {
                    return Err(Diagnostic::error("settings::merge", "merge must be replace or append")
                        .at(a.span, "bad merge"));
                }
            },
        };
        Ok(serde_json::to_value(Setting { layers, merge }).unwrap())
    }

    fn check(&self, schema: &Schema) -> Vec<Diagnostic> {
        let layers: Vec<Layer> = schema.ext_as("settings").unwrap();
        let mut out = Vec::new();
        for g in &schema.keys {
            for k in &g.keys {
                for s in ext_as::<Setting>(&k.ext, "settings").unwrap() {
                    for l in s.layers.iter().filter(|l| !layers.iter().any(|x| &x.name == *l)) {
                        out.push(
                            Diagnostic::error("settings::unknown_layer", format!("unknown layer `{l}`"))
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
layer "user"
layer "project"

keys "session" {
  key "steer.enabled" "bool" default=#false {
    setting layers="user project" merge="replace"
  }
  key "timeout_s" "u64" default=600
}
"#;

fn load(src: &str) -> Result<korek_core::Loaded, korek_core::Failure> {
    Loader::new().extension(Settings).load_sources(
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
        ext_as::<Setting>(&key.ext, "settings").unwrap(),
        [Setting {
            layers: vec!["user".into(), "project".into()],
            merge: "replace".into()
        }]
    );
    // Through the plugin protocol's encoding and back.
    let req = GenerateRequest::new("any", schema);
    let enc = korek_plugin::Encoding::Json;
    let back: GenerateRequest = enc.decode(&enc.encode(&req).unwrap()).unwrap();
    assert_eq!(back, req);
    assert_eq!(back.schema.ext_as::<Layer>("settings").unwrap().len(), 2);
}

#[test]
fn extension_errors_have_spans() {
    let f = load("keys \"s\" { key \"a\" \"bool\" { setting merge=\"mix\" } }").unwrap_err();
    assert!(f.has("settings::merge"), "{f}");
    assert!(f.diagnostics[0].span.is_some());
    let f = load("keys \"s\" { key \"a\" \"bool\" { setting layers=\"nope\" } }").unwrap_err();
    assert!(f.has("settings::unknown_layer"), "{f}");
    // Without the extension, the nodes are unknown.
    let f = Loader::new()
        .load_sources(vec![SourceFile { path: "s.kdl".into(), contents: SCHEMA.into() }], None)
        .unwrap_err();
    assert!(f.has("korek::unknown_node"), "{f}");
}

/// Adds a `LAYERS` const after each key with a setting, and a file.
struct LayerConsts;

impl Hook for LayerConsts {
    fn after(&self, item: Item<'_>, out: &mut String) {
        if let Item::Key(_, k) = item {
            for s in ext_as::<Setting>(&k.ext, "settings").unwrap() {
                let layers: Vec<String> = s.layers.iter().map(|l| format!("{l:?}")).collect();
                out.push_str(&format!(
                    "    pub const {}_LAYERS: &[&str] = &[{}];\n",
                    k.ident().to_uppercase(),
                    layers.join(", ")
                ));
            }
        }
    }

    fn files(&self, schema: &Schema) -> Vec<File> {
        let n = schema.ext_as::<Layer>("settings").unwrap().len();
        vec![File::new("layers.txt", format!("{n} layers\n"))]
    }
}

#[test]
fn generator_hooks_add_output() {
    let schema = load(SCHEMA).unwrap().schema;
    let files = korek_gen_rust::generate(&schema, &korek_gen_rust::Options::default(), &[&LayerConsts]);
    assert_eq!(files.len(), 2);
    assert!(
        files[0]
            .content
            .contains("pub const STEER_ENABLED_LAYERS: &[&str] = &[\"user\", \"project\"];"),
        "{}",
        files[0].content
    );
    assert_eq!(files[1], File::new("layers.txt", "2 layers\n"));
}
