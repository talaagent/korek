//! `korek-gen-example`: a third-party korek plugin in a few lines. It writes
//! `keys.md`, a table of every key, including any extension metadata.

use core::fmt::Write as _;

use korek_plugin::{File, GenerateResponse};

fn main() -> std::process::ExitCode {
    korek_plugin::run(|req| {
        let mut md =
            String::from("| Key | Type | Default | Doc | Metadata |\n|---|---|---|---|---|\n");
        for g in &req.schema.keys {
            for k in &g.keys {
                let default = k
                    .default
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                let meta = if k.ext.is_empty() {
                    String::new()
                } else {
                    serde_json::to_string(&k.ext).unwrap_or_default()
                };
                let _ = writeln!(
                    md,
                    "| `{}.{}` | `{}` | `{default}` | {} | {meta} |",
                    g.name,
                    k.name,
                    k.ty.display(),
                    k.doc.as_deref().unwrap_or("")
                );
            }
        }
        Ok(GenerateResponse::new(vec![File::new("keys.md", md)]))
    })
}
