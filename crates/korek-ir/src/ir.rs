//! The IR: pure data, no logic (see `crate::impls` for helpers).
//!
//! It travels to plugins inside a `GenerateRequest`, so it is versioned
//! with the plugin protocol. Only constructs korek can itself express are
//! used (structs, enums, tagged unions, lists, maps, optionals, primitives), so the IR can
//! later be defined in a korek schema. Structs are plain (constructible by
//! other crates); rule and format enums are `#[non_exhaustive]`.

use alloc::{boxed::Box, collections::BTreeMap, string::String, vec::Vec};

use serde::{Deserialize, Serialize};

extern crate alloc;

/// A JSON value (defaults, examples, rule operands, metadata).
pub type Json = serde_json::Value;

/// Extension metadata attached to an item, by extension name: one value
/// per node or property the extension handled.
pub type Ext = BTreeMap<String, Vec<Json>>;

/// A location in one of the schema's source files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Span {
    /// Index into [`Schema::files`].
    pub file: u32,
    /// Byte offset of the start.
    pub offset: u32,
    /// Length in bytes.
    pub len: u32,
}

/// A whole schema: every `.kdl` file of a directory, lowered and validated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schema {
    /// The Protobuf-style package (`package "acme.config.v1"`), `korek.v1`
    /// when the schema has none.
    pub package: String,
    /// Source files, relative to the schema directory, in load order.
    pub files: Vec<String>,
    pub types: Vec<TypeDef>,
    pub keys: Vec<KeyGroup>,
    pub services: Vec<Service>,
    /// Top-level extension nodes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ext: Ext,
}

/// Documentation and metadata every item can carry.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Attrs {
    /// One line (`doc "…"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// Long markdown (`description """…"""`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Example values, checked against the type and its rules.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<Json>,
    /// `unit "seconds"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// `deprecated "use X"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    /// Former or alternative names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// `since "1.4"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    /// Never printed or rendered with its value.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub secret: bool,
    /// UI hints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<Ui>,
    /// Namespaced metadata passed through untouched (`meta "acme.ui" k=v`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meta: Vec<Meta>,
    /// Extension metadata.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ext: Ext,
}

/// UI hints for forms and docs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ui {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<Level>,
    /// e.g. `textarea`, `select`, `toggle`, `duration`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widget: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Basic,
    Advanced,
}

/// `meta "<namespace>" key=value …`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    pub namespace: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub props: BTreeMap<String, Json>,
}

/// A named type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeDef {
    pub name: String,
    pub kind: TypeKind,
    #[serde(default)]
    pub attrs: Attrs,
    /// Message-level rules (`required` on unions, `cel` on structs).
    #[serde(default, skip_serializing_if = "Validate::is_empty")]
    pub validate: Validate,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeKind {
    Enum(EnumDef),
    Struct(StructDef),
    Union(UnionDef),
}

/// A closed set of names, serialized as the names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumDef {
    pub variants: Vec<EnumVariant>,
    /// Numbers of removed variants, never reused.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retired: Vec<Retired>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumVariant {
    /// `lower_snake_case`, also the serialized form.
    pub name: String,
    /// Protobuf value number (`>= 1`; 0 is `UNSPECIFIED`).
    pub number: u32,
    #[serde(default)]
    pub attrs: Attrs,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructDef {
    pub fields: Vec<Field>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retired: Vec<Retired>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    /// `lower_snake_case`, also the serialized name.
    pub name: String,
    pub ty: Type,
    pub optional: bool,
    /// The default in canonical JSON, checked against type and rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Json>,
    #[serde(default, skip_serializing_if = "Validate::is_empty")]
    pub validate: Validate,
    /// Protobuf field number.
    pub number: u32,
    #[serde(default)]
    pub attrs: Attrs,
    pub span: Span,
}

/// One of several types: tagged (`tag` names the field holding the variant
/// name; variants are structs) or untagged (the first variant that matches).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnionDef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    pub variants: Vec<UnionVariant>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retired: Vec<Retired>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnionVariant {
    pub name: String,
    pub ty: Type,
    /// Protobuf field number in the `oneof`.
    pub number: u32,
    #[serde(default)]
    pub attrs: Attrs,
    pub span: Span,
}

/// A removed item whose number stays reserved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retired {
    pub name: String,
    pub number: u32,
}

/// A `keys` block: named, typed keys with defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyGroup {
    /// Dotted `lower_snake_case` (`session`, `ui.theme`).
    pub name: String,
    pub keys: Vec<Key>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retired: Vec<Retired>,
    #[serde(default)]
    pub attrs: Attrs,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Key {
    /// Dotted `lower_snake_case` within the block (`steer.enabled`).
    pub name: String,
    pub ty: Type,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Json>,
    #[serde(default, skip_serializing_if = "Validate::is_empty")]
    pub validate: Validate,
    /// Protobuf field number in the block's `<Block>Keys` message.
    pub number: u32,
    #[serde(default)]
    pub attrs: Attrs,
    pub span: Span,
}

/// An RPC service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Service {
    pub name: String,
    pub rpcs: Vec<Rpc>,
    #[serde(default)]
    pub attrs: Attrs,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rpc {
    pub name: String,
    /// A struct or union type name.
    pub input: String,
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency: Option<Idempotency>,
    #[serde(default)]
    pub attrs: Attrs,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Idempotency {
    NoSideEffects,
    Idempotent,
}

/// A type. JSON forms: integers as numbers, `bytes` base64, `timestamp`
/// RFC 3339, `duration` a decimal number of seconds with an `s` suffix
/// (`"1.5s"`), as in the Protobuf JSON mapping; `json` any JSON value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Type {
    Bool,
    String,
    Bytes,
    I32,
    I64,
    U32,
    U64,
    F32,
    F64,
    Timestamp,
    Duration,
    /// Any JSON value. Added in protocol 2 without a bump: schemas that
    /// do not use it serialize as before.
    Json,
    List(Box<Type>),
    /// `map<string,T>`.
    Map(Box<Type>),
    /// A reference to a [`TypeDef`].
    Named(String),
}

/// The `validate { … }` block of a field, key, type or element.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Validate {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<Rule>,
    /// `items { … }`: rules for each list item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<Validate>>,
    /// `keys { … }`: rules for each map key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys: Option<Box<Validate>>,
    /// `values { … }`: rules for each map value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<Box<Validate>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub kind: RuleKind,
    /// A custom error message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub span: Span,
}

/// One validation rule. See `VALIDATION.md` for what each target supports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "rule", rename_all = "snake_case")]
#[non_exhaustive]
pub enum RuleKind {
    /// Equal to a value.
    Const {
        value: Json,
    },
    /// One of the values.
    In {
        values: Vec<Json>,
    },
    NotIn {
        values: Vec<Json>,
    },
    /// Length in characters (strings), bytes (bytes).
    Len {
        n: u64,
    },
    MinLen {
        n: u64,
    },
    MaxLen {
        n: u64,
    },
    /// Length of a string in UTF-8 bytes.
    LenBytes {
        n: u64,
    },
    MinBytes {
        n: u64,
    },
    MaxBytes {
        n: u64,
    },
    /// A regular expression (RE2 syntax) that must match somewhere.
    Pattern {
        regex: String,
    },
    Prefix {
        value: String,
    },
    Suffix {
        value: String,
    },
    Contains {
        value: String,
    },
    NotContains {
        value: String,
    },
    /// A well-known string format.
    Format {
        format: Format,
    },
    Gt {
        value: Json,
    },
    Gte {
        value: Json,
    },
    Lt {
        value: Json,
    },
    Lte {
        value: Json,
    },
    /// Not infinite or NaN (floats).
    Finite,
    MultipleOf {
        value: Json,
    },
    MinItems {
        n: u64,
    },
    MaxItems {
        n: u64,
    },
    /// No duplicate list items.
    Unique,
    MinPairs {
        n: u64,
    },
    MaxPairs {
        n: u64,
    },
    /// A message or union must be set; an enum must not be unspecified.
    Required,
    /// Enum values must be declared variants (always true in JSON).
    DefinedOnly,
    /// Timestamps relative to now.
    GtNow,
    LtNow,
    /// Within a duration of now (`"3600s"`).
    Within {
        duration: String,
    },
    /// A protovalidate CEL expression, with optional equivalents for other
    /// targets.
    Cel {
        id: String,
        expression: String,
        /// A TypeScript predicate `(v) => boolean`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ts: Option<String>,
        /// A Rust expression over `v` (a reference to the value) that is
        /// `true` when valid.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rust: Option<String>,
    },
}

/// Well-known string formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Format {
    Email,
    Hostname,
    Ip,
    Ipv4,
    Ipv6,
    /// An IP address with a prefix length (`10.0.0.1/8`).
    IpWithPrefixlen,
    /// A network prefix (`10.0.0.0/8`).
    Cidr,
    Uri,
    UriRef,
    Url,
    Uuid,
    /// A UUID without dashes.
    Tuuid,
    Ulid,
    HostAndPort,
    /// A hostname or IP address.
    Address,
    Cuid,
    Cuid2,
    Nanoid,
    Datetime,
    Date,
    Time,
    /// ISO 8601 duration (`P1DT2H`).
    Duration,
    Base64,
    Base64url,
    Jwt,
    E164,
    Emoji,
    Lowercase,
    Uppercase,
}
