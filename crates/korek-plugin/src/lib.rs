//! The korek generator plugin protocol.
//!
//! The `korek` driver loads and validates a schema, then runs one plugin
//! binary per output, `korek-gen-<name>` (like protoc's `protoc-gen-*`). It
//! writes a [`GenerateRequest`] to the plugin's stdin and reads a
//! [`GenerateResponse`] from its stdout. Both are JSON in protocol version 1
//! ([`Encoding`]); the driver names the encoding in `KOREK_ENCODING`.
//!
//! A plugin is a `main` over [`run`]:
//!
//! ```no_run
//! use korek_plugin::{File, GenerateResponse};
//!
//! fn main() -> std::process::ExitCode {
//!     korek_plugin::run(|req| {
//!         let names: Vec<_> = req.schema.types.iter().map(|t| t.name.as_str()).collect();
//!         Ok(GenerateResponse::new(vec![File::new("types.txt", names.join("\n"))]))
//!     })
//! }
//! ```
//!
//! Generators also take [`Hook`]s when called in-process, so an extension can
//! add output (extra items after a type, extra files) without forking them.

use alloc::collections::BTreeMap;
use std::io::{Read as _, Write as _};
use std::process::ExitCode;

pub use korek_core::Diagnostic;
pub use korek_core::ir::{Key, KeyGroup, Schema, Service, TypeDef};
use serde::{Deserialize, Serialize};

extern crate alloc;

/// The protocol version this crate speaks. Bumped on breaking changes to
/// the request, the response or the IR.
pub const PROTOCOL_VERSION: u32 = 1;

/// The environment variable naming the encoding (`json`).
pub const ENCODING_ENV: &str = "KOREK_ENCODING";

/// What the driver sends a plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct GenerateRequest {
    pub protocol_version: u32,
    /// The driver's version.
    pub korek_version: String,
    /// The plugin's name (`rust` for `korek-gen-rust`).
    pub plugin: String,
    /// `--<name>_opt k=v,…` and `--opt <name>:k=v`.
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    /// The validated schema, extension metadata included.
    pub schema: Schema,
}

impl GenerateRequest {
    pub fn new(plugin: impl Into<String>, schema: Schema) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            korek_version: env!("CARGO_PKG_VERSION").into(),
            plugin: plugin.into(),
            options: BTreeMap::new(),
            schema,
        }
    }

    #[must_use]
    pub fn with_options(mut self, options: BTreeMap<String, String>) -> Self {
        self.options = options;
        self
    }

    /// A boolean option (`true`/`false`), `default` when unset.
    ///
    /// # Errors
    /// When the value is neither.
    pub fn bool_option(&self, name: &str, default: bool) -> Result<bool, Diagnostic> {
        match self.options.get(name).map(String::as_str) {
            None => Ok(default),
            Some("true" | "1" | "yes") => Ok(true),
            Some("false" | "0" | "no") => Ok(false),
            Some(v) => Err(Diagnostic::error(
                "korek::bad_option",
                format!("option `{name}` must be true or false, found `{v}`"),
            )),
        }
    }

    /// Fails on options not in `known`.
    ///
    /// # Errors
    /// A diagnostic naming the first unknown option.
    pub fn check_options(&self, known: &[&str]) -> Result<(), Diagnostic> {
        match self.options.keys().find(|k| !known.contains(&k.as_str())) {
            Some(k) => Err(Diagnostic::error(
                "korek::bad_option",
                format!(
                    "unknown option `{k}` for korek-gen-{} (known: {})",
                    self.plugin,
                    known.join(", ")
                ),
            )),
            None => Ok(()),
        }
    }
}

/// A generated file, relative to the plugin's output directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
    pub path: String,
    pub content: String,
}

impl File {
    pub fn new(path: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            content: content.into(),
        }
    }
}

/// What a plugin answers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct GenerateResponse {
    pub files: Vec<File>,
    /// Errors fail the run; warnings are printed.
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

impl GenerateResponse {
    pub fn new(files: Vec<File>) -> Self {
        Self {
            files,
            diagnostics: Vec::new(),
        }
    }

    /// A response carrying one error.
    pub fn error(d: Diagnostic) -> Self {
        Self {
            files: Vec::new(),
            diagnostics: vec![d],
        }
    }
}

/// How requests and responses are encoded on the pipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Encoding {
    #[default]
    Json,
}

impl Encoding {
    pub fn name(self) -> &'static str {
        match self {
            Self::Json => "json",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        (name == "json").then_some(Self::Json)
    }

    /// # Errors
    /// When serialization fails.
    pub fn encode<T: Serialize>(self, v: &T) -> Result<Vec<u8>, String> {
        match self {
            Self::Json => serde_json::to_vec(v).map_err(|e| e.to_string()),
        }
    }

    /// # Errors
    /// When the bytes are not a valid `T`.
    pub fn decode<T: serde::de::DeserializeOwned>(self, bytes: &[u8]) -> Result<T, String> {
        match self {
            Self::Json => serde_json::from_slice(bytes).map_err(|e| e.to_string()),
        }
    }
}

/// Where a generator is in its output, for [`Hook::after`].
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum Item<'a> {
    Type(&'a TypeDef),
    KeyGroup(&'a KeyGroup),
    Key(&'a KeyGroup, &'a Key),
    Service(&'a Service),
}

/// Adds output to a generator called in-process.
pub trait Hook {
    /// Text to write right after `item` (a type, a keys block, a key or a
    /// service), in the generator's language.
    fn after(&self, item: Item<'_>, out: &mut String) {
        let _ = (item, out);
    }

    /// Text to write at the end of the main file.
    fn footer(&self, schema: &Schema, out: &mut String) {
        let _ = (schema, out);
    }

    /// Extra files.
    fn files(&self, schema: &Schema) -> Vec<File> {
        let _ = schema;
        Vec::new()
    }
}

/// Runs a plugin: reads the request from stdin, calls `f`, writes the
/// response to stdout. An `Err` becomes a response with that error.
pub fn run(f: impl FnOnce(&GenerateRequest) -> Result<GenerateResponse, Diagnostic>) -> ExitCode {
    let name = std::env::var(ENCODING_ENV).unwrap_or_else(|_| "json".into());
    let Some(enc) = Encoding::from_name(&name) else {
        eprintln!("unsupported encoding {name:?}");
        return ExitCode::FAILURE;
    };
    let mut input = Vec::new();
    if let Err(e) = std::io::stdin().read_to_end(&mut input) {
        eprintln!("reading the request: {e}");
        return ExitCode::FAILURE;
    }
    let req: GenerateRequest = match enc.decode(&input) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("decoding the request: {e}");
            return ExitCode::FAILURE;
        }
    };
    let resp = if req.protocol_version == PROTOCOL_VERSION {
        f(&req).unwrap_or_else(GenerateResponse::error)
    } else {
        GenerateResponse::error(Diagnostic::error(
            "korek::protocol",
            format!(
                "plugin speaks protocol {PROTOCOL_VERSION}, driver sent {}",
                req.protocol_version
            ),
        ))
    };
    let out = match enc.encode(&resp) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("encoding the response: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut stdout = std::io::stdout().lock();
    if stdout
        .write_all(&out)
        .and_then(|()| stdout.flush())
        .is_err()
    {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Shared text helpers for generators.
pub mod text {
    /// Prefixes each line of `doc` with `prefix` (`/// `, `// `, ` * `).
    pub fn doc_lines(out: &mut String, indent: &str, prefix: &str, doc: Option<&str>) {
        let Some(doc) = doc else { return };
        for line in doc.lines() {
            out.push_str(indent);
            if line.is_empty() {
                out.push_str(prefix.trim_end());
            } else {
                out.push_str(prefix);
                out.push_str(line);
            }
            out.push('\n');
        }
    }

    /// The header line for generated files.
    pub const GENERATED: &str = "Generated by korek from the schema. Do not edit.";
}
