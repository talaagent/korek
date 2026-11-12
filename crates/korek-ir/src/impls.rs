//! Helpers over the pure-data IR and protocol types.

use crate::ir::{Ext, Format, Key, KeyGroup, RuleKind, Schema, Span, Type, TypeDef, Validate};
use crate::protocol::{Diagnostic, File, GenerateRequest, GenerateResponse, Label, Severity};

/// Looks up named types: [`Schema`] scans its list; [`TypeIndex`] hashes.
pub trait Types {
    /// The named type `name`, if declared.
    fn get(&self, name: &str) -> Option<&TypeDef>;
}

impl Types for Schema {
    fn get(&self, name: &str) -> Option<&TypeDef> {
        self.types.iter().find(|t| t.name == name)
    }
}

/// Constant-time type lookup by name. Build it once per pass over a large
/// schema ([`Schema::index`]).
#[derive(Debug, Clone)]
pub struct TypeIndex<'a> {
    by_name: std::collections::HashMap<&'a str, &'a TypeDef>,
}

impl<'a> TypeIndex<'a> {
    /// [`Types::get`], borrowing from the schema rather than the index.
    pub fn def(&self, name: &str) -> Option<&'a TypeDef> {
        self.by_name.get(name).copied()
    }
}

impl Types for TypeIndex<'_> {
    fn get(&self, name: &str) -> Option<&TypeDef> {
        self.by_name.get(name).copied()
    }
}

impl Schema {
    /// The named type `name`, if declared.
    pub fn get(&self, name: &str) -> Option<&TypeDef> {
        Types::get(self, name)
    }

    /// A [`TypeIndex`] over this schema's types.
    pub fn index(&self) -> TypeIndex<'_> {
        TypeIndex {
            by_name: self.types.iter().map(|t| (t.name.as_str(), t)).collect(),
        }
    }

    /// Deserializes every value extension `name` attached at the top level.
    ///
    /// # Errors
    /// When a value does not match `T`.
    pub fn ext_as<T: serde::de::DeserializeOwned>(
        &self,
        name: &str,
    ) -> Result<Vec<T>, serde_json::Error> {
        ext_as(&self.ext, name)
    }
}

/// Deserializes the values extension `name` attached to an item.
///
/// # Errors
/// When a value does not match `T`.
pub fn ext_as<T: serde::de::DeserializeOwned>(
    ext: &Ext,
    name: &str,
) -> Result<Vec<T>, serde_json::Error> {
    ext.get(name)
        .into_iter()
        .flatten()
        .map(|v| serde_json::from_value(v.clone()))
        .collect()
}

impl Key {
    /// `steer.enabled` → `steer_enabled`.
    pub fn ident(&self) -> String {
        self.name.replace('.', "_")
    }
}

impl KeyGroup {
    /// `ui.theme` → `ui_theme`.
    pub fn ident(&self) -> String {
        self.name.replace('.', "_")
    }

    /// `session` + `steer_enabled` → `session_steer_enabled`: a key's stable
    /// identifier across the whole schema.
    pub fn key_ident(&self, key: &Key) -> String {
        format!("{}_{}", self.ident(), key.ident())
    }
}

impl Type {
    /// The type as written in a schema (`map<string,list<i64>>`).
    pub fn display(&self) -> String {
        match self {
            Self::List(t) => format!("list<{}>", t.display()),
            Self::Map(t) => format!("map<string,{}>", t.display()),
            Self::Named(n) => n.clone(),
            other => other.primitive_name().unwrap_or("?").to_owned(),
        }
    }

    /// The schema spelling of a primitive.
    pub fn primitive_name(&self) -> Option<&'static str> {
        Some(match self {
            Self::Bool => "bool",
            Self::String => "string",
            Self::Bytes => "bytes",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::Timestamp => "timestamp",
            Self::Duration => "duration",
            _ => return None,
        })
    }

    /// Parses a primitive name.
    pub fn primitive(name: &str) -> Option<Self> {
        Some(match name {
            "bool" => Self::Bool,
            "string" => Self::String,
            "bytes" => Self::Bytes,
            "i32" => Self::I32,
            "i64" => Self::I64,
            "u32" => Self::U32,
            "u64" => Self::U64,
            "f32" => Self::F32,
            "f64" => Self::F64,
            "timestamp" => Self::Timestamp,
            "duration" => Self::Duration,
            _ => return None,
        })
    }

    pub fn is_integer(&self) -> bool {
        matches!(self, Self::I32 | Self::I64 | Self::U32 | Self::U64)
    }

    pub fn is_float(&self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }

    pub fn is_numeric(&self) -> bool {
        self.is_integer() || self.is_float()
    }

    pub fn is_unsigned(&self) -> bool {
        matches!(self, Self::U32 | Self::U64)
    }

    pub fn is_collection(&self) -> bool {
        matches!(self, Self::List(_) | Self::Map(_))
    }
}

impl Validate {
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
            && self.items.is_none()
            && self.keys.is_none()
            && self.values.is_none()
    }

    /// Whether a rule of this kind is present.
    pub fn has(&self, f: impl Fn(&RuleKind) -> bool) -> bool {
        self.rules.iter().any(|r| f(&r.kind))
    }
}

impl RuleKind {
    /// The rule's name as written in `validate { … }`.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Const { .. } => "const",
            Self::In { .. } => "in",
            Self::NotIn { .. } => "not_in",
            Self::Len { .. } => "len",
            Self::MinLen { .. } => "min_len",
            Self::MaxLen { .. } => "max_len",
            Self::LenBytes { .. } => "len_bytes",
            Self::MinBytes { .. } => "min_bytes",
            Self::MaxBytes { .. } => "max_bytes",
            Self::Pattern { .. } => "pattern",
            Self::Prefix { .. } => "prefix",
            Self::Suffix { .. } => "suffix",
            Self::Contains { .. } => "contains",
            Self::NotContains { .. } => "not_contains",
            Self::Format { format } => format.name(),
            Self::Gt { .. } => "gt",
            Self::Gte { .. } => "gte",
            Self::Lt { .. } => "lt",
            Self::Lte { .. } => "lte",
            Self::Finite => "finite",
            Self::MultipleOf { .. } => "multiple_of",
            Self::MinItems { .. } => "min_items",
            Self::MaxItems { .. } => "max_items",
            Self::Unique => "unique",
            Self::MinPairs { .. } => "min_pairs",
            Self::MaxPairs { .. } => "max_pairs",
            Self::Required => "required",
            Self::DefinedOnly => "defined_only",
            Self::GtNow => "gt_now",
            Self::LtNow => "lt_now",
            Self::Within { .. } => "within",
            Self::Cel { .. } => "cel",
        }
    }
}

impl Format {
    /// Every format, for parsing and docs.
    pub const ALL: &'static [Self] = &[
        Self::Email,
        Self::Hostname,
        Self::Ip,
        Self::Ipv4,
        Self::Ipv6,
        Self::IpWithPrefixlen,
        Self::Cidr,
        Self::Uri,
        Self::UriRef,
        Self::Url,
        Self::Uuid,
        Self::Tuuid,
        Self::Ulid,
        Self::HostAndPort,
        Self::Address,
        Self::Cuid,
        Self::Cuid2,
        Self::Nanoid,
        Self::Datetime,
        Self::Date,
        Self::Time,
        Self::Duration,
        Self::Base64,
        Self::Base64url,
        Self::Jwt,
        Self::E164,
        Self::Emoji,
        Self::Lowercase,
        Self::Uppercase,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Hostname => "hostname",
            Self::Ip => "ip",
            Self::Ipv4 => "ipv4",
            Self::Ipv6 => "ipv6",
            Self::IpWithPrefixlen => "ip_with_prefixlen",
            Self::Cidr => "cidr",
            Self::Uri => "uri",
            Self::UriRef => "uri_ref",
            Self::Url => "url",
            Self::Uuid => "uuid",
            Self::Tuuid => "tuuid",
            Self::Ulid => "ulid",
            Self::HostAndPort => "host_and_port",
            Self::Address => "address",
            Self::Cuid => "cuid",
            Self::Cuid2 => "cuid2",
            Self::Nanoid => "nanoid",
            Self::Datetime => "datetime",
            Self::Date => "date",
            Self::Time => "time",
            Self::Duration => "iso_duration",
            Self::Base64 => "base64",
            Self::Base64url => "base64url",
            Self::Jwt => "jwt",
            Self::E164 => "e164",
            Self::Emoji => "emoji",
            Self::Lowercase => "lowercase",
            Self::Uppercase => "uppercase",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|f| f.name() == name)
    }
}

impl Diagnostic {
    pub fn error(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            severity: Severity::Error,
            message: message.into(),
            help: None,
            labels: Vec::new(),
        }
    }

    pub fn warning(code: &str, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::error(code, message)
        }
    }

    /// Adds a labeled location (the first is the primary one).
    #[must_use]
    pub fn at(mut self, span: Span, text: impl Into<String>) -> Self {
        self.labels.push(Label {
            span,
            text: Some(text.into()),
        });
        self
    }

    #[must_use]
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// The primary span.
    pub fn span(&self) -> Option<Span> {
        self.labels.first().map(|l| l.span)
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

impl File {
    pub fn new(path: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            content: content.into(),
        }
    }
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

    #[must_use]
    pub fn with_diagnostics(mut self, d: Vec<Diagnostic>) -> Self {
        self.diagnostics.extend(d);
        self
    }
}

impl GenerateRequest {
    pub fn new(plugin: impl Into<String>, schema: Schema) -> Self {
        Self {
            protocol_version: crate::PROTOCOL_VERSION,
            korek_version: env!("CARGO_PKG_VERSION").into(),
            plugin: plugin.into(),
            options: alloc::collections::BTreeMap::default(),
            schema,
        }
    }

    #[must_use]
    pub fn with_options(mut self, options: alloc::collections::BTreeMap<String, String>) -> Self {
        self.options = options;
        self
    }

    /// A boolean option, `default` when unset.
    ///
    /// # Errors
    /// When the value is not a boolean.
    pub fn bool_option(&self, name: &str, default: bool) -> Result<bool, Diagnostic> {
        match self.options.get(name).map(String::as_str) {
            None => Ok(default),
            Some("true" | "1" | "yes") => Ok(true),
            Some("false" | "0" | "no") => Ok(false),
            Some(v) => Err(Diagnostic::error(
                crate::codes::INVALID_OPTION,
                format!("option `{name}` must be true or false, found `{v}`"),
            )),
        }
    }

    /// Fails on options not in `known`.
    ///
    /// # Errors
    /// Naming the first unknown option.
    pub fn check_options(&self, known: &[&str]) -> Result<(), Diagnostic> {
        match self.options.keys().find(|k| !known.contains(&k.as_str())) {
            Some(k) => Err(Diagnostic::error(
                crate::codes::INVALID_OPTION,
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

/// Formats a JSON number or bound without a trailing `.0` when whole.
pub fn format_number(n: f64) -> String {
    #[allow(clippy::cast_possible_truncation)]
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}
