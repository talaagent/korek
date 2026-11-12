//! The plugin protocol: what the driver sends a generator plugin and what
//! it answers. Pure data.

use alloc::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ir::{Schema, Span};

extern crate alloc;

/// What the driver sends a plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerateRequest {
    /// [`crate::PROTOCOL_VERSION`] of the driver.
    pub protocol_version: u32,
    pub korek_version: String,
    /// The plugin's name (`rust` for `korek-gen-rust`).
    pub plugin: String,
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    /// The validated schema, extension metadata included.
    pub schema: Schema,
}

/// A generated file, relative to the plugin's output directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
    pub path: String,
    pub content: String,
}

/// What a plugin answers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GenerateResponse {
    pub files: Vec<File>,
    /// Errors fail the run; warnings are reported.
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    #[default]
    Error,
    Warning,
}

/// A problem, from the loader or a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Stable and namespaced: `korek::unknown_type`,
    /// `korek_gen_proto::…`.
    pub code: String,
    #[serde(default)]
    pub severity: Severity,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    /// The first label is the primary location.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<Label>,
}

/// A labeled source location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    pub span: Span,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}
