//! The intermediate representation every generator reads.
//!
//! These types are korek's stable interface: they travel to plugins as JSON
//! inside a `GenerateRequest`, so changes to them are versioned with the
//! plugin protocol. Structs are `#[non_exhaustive]` so new fields can be
//! added without breaking code that reads them.

use alloc::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Extension metadata attached to an IR item, keyed by extension name. Each
/// extension contributes one JSON value per node or property it handled.
pub type Ext = BTreeMap<String, Vec<serde_json::Value>>;

/// A location in one of the schema's source files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
#[non_exhaustive]
pub struct Schema {
    /// The Protobuf-style package (`package "acme.config.v1"`), `korek.v1`
    /// when the schema has none.
    pub package: String,
    /// Source files, relative to the schema directory, in load order.
    pub files: Vec<String>,
    /// Named types in declaration order.
    pub types: Vec<TypeDef>,
    /// `keys` blocks in declaration order.
    pub keys: Vec<KeyGroup>,
    /// RPC services in declaration order.
    pub services: Vec<Service>,
    /// Top-level extension nodes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ext: Ext,
}

impl Schema {
    /// The named type `name`, if declared.
    pub fn get(&self, name: &str) -> Option<&TypeDef> {
        self.types.iter().find(|t| t.name == name)
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

/// A named type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TypeDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    pub kind: TypeKind,
    pub span: Span,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ext: Ext,
}

/// What a named type is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TypeKind {
    Enum(EnumDef),
    Struct(StructDef),
    Union(UnionDef),
}

/// A closed set of named values, serialized as their names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EnumDef {
    pub variants: Vec<EnumVariant>,
    /// Numbers of removed variants, never reused.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retired: Vec<Retired>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EnumVariant {
    /// `lower_snake_case`, also the serialized form.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// Protobuf value number (`>= 1`; 0 is the `UNSPECIFIED` value).
    pub number: u32,
    pub span: Span,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ext: Ext,
}

/// A record of named fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct StructDef {
    pub fields: Vec<Field>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retired: Vec<Retired>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Field {
    /// `lower_snake_case`, also the serialized name.
    pub name: String,
    pub ty: Type,
    /// May be absent.
    pub optional: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    #[serde(default, skip_serializing_if = "Constraints::is_empty")]
    pub constraints: Constraints,
    /// Protobuf field number.
    pub number: u32,
    pub span: Span,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ext: Ext,
}

/// One of several types. Tagged unions (`tag` set) have struct variants and
/// carry the variant name in the tag field; untagged unions are the first
/// variant whose type matches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UnionDef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    pub variants: Vec<UnionVariant>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retired: Vec<Retired>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UnionVariant {
    /// `lower_snake_case`; the tag value of a tagged union.
    pub name: String,
    pub ty: Type,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// Protobuf field number inside the `oneof`.
    pub number: u32,
    pub span: Span,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ext: Ext,
}

/// A removed field, variant or key whose number stays reserved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retired {
    pub name: String,
    pub number: u32,
}

/// A `keys` block: named, typed keys with defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct KeyGroup {
    /// Dotted `lower_snake_case` segments (`session`, `ui.theme`).
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    pub keys: Vec<Key>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retired: Vec<Retired>,
    pub span: Span,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ext: Ext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Key {
    /// Dotted `lower_snake_case` segments (`steer.enabled`).
    pub name: String,
    pub ty: Type,
    /// The default in its JSON form (what serde and Zod read), already
    /// checked against `ty` and `constraints`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    #[serde(default, skip_serializing_if = "Constraints::is_empty")]
    pub constraints: Constraints,
    /// Protobuf field number in the group's `<Group>Keys` message.
    pub number: u32,
    pub span: Span,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ext: Ext,
}

impl Key {
    /// `steer.enabled` → `steer_enabled`: the key as one identifier.
    pub fn ident(&self) -> String {
        self.name.replace('.', "_")
    }
}

impl KeyGroup {
    /// `ui.theme` → `ui_theme`.
    pub fn ident(&self) -> String {
        self.name.replace('.', "_")
    }
}

/// An RPC service (emitted to Protobuf for ConnectRPC/gRPC tooling).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Service {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    pub rpcs: Vec<Rpc>,
    pub span: Span,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ext: Ext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Rpc {
    pub name: String,
    /// A struct or union type name.
    pub input: String,
    /// A struct or union type name.
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency: Option<Idempotency>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    pub span: Span,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ext: Ext,
}

/// Protobuf's `idempotency_level`; `no_side_effects` lets Connect use GET.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Idempotency {
    NoSideEffects,
    Idempotent,
}

/// A field, variant or key type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Type {
    Bool,
    String,
    I64,
    U64,
    F64,
    /// Serialized as an array of byte values.
    Bytes,
    List(Box<Type>),
    /// `map<string,T>`: string keys only.
    Map(Box<Type>),
    /// A reference to a [`TypeDef`].
    Named(String),
}

impl Type {
    /// The type as written in a schema (`map<string,list<i64>>`).
    pub fn display(&self) -> String {
        match self {
            Self::Bool => "bool".into(),
            Self::String => "string".into(),
            Self::I64 => "i64".into(),
            Self::U64 => "u64".into(),
            Self::F64 => "f64".into(),
            Self::Bytes => "bytes".into(),
            Self::List(t) => format!("list<{}>", t.display()),
            Self::Map(t) => format!("map<string,{}>", t.display()),
            Self::Named(n) => n.clone(),
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::I64 | Self::U64 | Self::F64)
    }

    /// Strings, bytes, lists and maps have a length.
    pub fn has_len(&self) -> bool {
        matches!(
            self,
            Self::String | Self::Bytes | Self::List(_) | Self::Map(_)
        )
    }

    pub fn is_collection(&self) -> bool {
        matches!(self, Self::List(_) | Self::Map(_))
    }
}

/// Value constraints on a field or key. Bounds are inclusive.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Constraints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Characters for strings, bytes for bytes, items for lists and maps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_len: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_len: Option<u64>,
    /// A regular expression the whole string must match somewhere in
    /// (unanchored, like protovalidate and Zod).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
}

impl Constraints {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Formats a bound without a trailing `.0` when it is whole (`86400`, `0.5`).
pub fn format_number(n: f64) -> String {
    #[allow(clippy::cast_possible_truncation)]
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}
