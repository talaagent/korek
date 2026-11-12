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

use std::io::{Read as _, Write as _};
use std::process::ExitCode;

pub use korek_ir as ir;
pub use korek_ir::protocol::{
    Diagnostic, File, GenerateRequest, GenerateResponse, Label, Severity,
};
pub use korek_ir::{Key, KeyGroup, PROTOCOL_VERSION, Schema, Service, TypeDef};
use serde::Serialize;

/// The environment variable naming the encoding (`json`).
pub const ENCODING_ENV: &str = "KOREK_ENCODING";

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
            korek_ir::codes::PLUGIN_PROTOCOL,
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
