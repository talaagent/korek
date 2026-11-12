//! Extensions: domain-specific nodes and properties.
//!
//! The core grammar rejects nodes and properties it does not know, unless a
//! registered [`Extension`] claims them. A claimed node or property is handed
//! to the extension, which returns a JSON value; korek stores it in the
//! `ext` map of the item it appeared on (or of the [`Schema`] for top-level
//! nodes) under the extension's name, and it travels with the IR to every
//! generator. Extensions define serde types for their metadata and read it
//! back with [`crate::ir::ext_as`].

use kdl::{KdlEntry, KdlNode, KdlValue};
use serde_json::Value;

use crate::diag::Diagnostic;
use crate::ir::{Schema, Span};

/// Where a claimed child node or property appeared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ItemKind {
    Enum,
    Struct,
    Union,
    EnumVariant,
    Field,
    UnionVariant,
    KeyGroup,
    Key,
    Service,
    Rpc,
}

/// A literal value from the schema source.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    String(String),
    Int(i128),
    Float(f64),
    Bool(bool),
    Null,
}

impl Literal {
    fn from_kdl(v: &KdlValue) -> Self {
        match v {
            KdlValue::String(s) => Self::String(s.clone()),
            KdlValue::Integer(i) => Self::Int(*i),
            KdlValue::Float(f) => Self::Float(*f),
            KdlValue::Bool(b) => Self::Bool(*b),
            KdlValue::Null => Self::Null,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// The natural JSON form. Integers outside `i64`/`u64` become floats.
    pub fn to_json(&self) -> Value {
        match self {
            Self::String(s) => Value::String(s.clone()),
            Self::Int(i) => {
                if let Ok(v) = i64::try_from(*i) {
                    Value::from(v)
                } else if let Ok(v) = u64::try_from(*i) {
                    Value::from(v)
                } else {
                    #[allow(clippy::cast_precision_loss)]
                    Value::from(*i as f64)
                }
            }
            Self::Float(f) => Value::from(*f),
            Self::Bool(b) => Value::Bool(*b),
            Self::Null => Value::Null,
        }
    }

    /// A short description for error messages.
    pub fn describe(&self) -> String {
        match self {
            Self::String(s) => format!("the string {s:?}"),
            Self::Int(i) => format!("the integer {i}"),
            Self::Float(f) => format!("the number {f}"),
            Self::Bool(b) => format!("#{b}"),
            Self::Null => "#null".into(),
        }
    }
}

/// An argument or property value with its span.
#[derive(Debug, Clone, PartialEq)]
pub struct Arg {
    pub value: Literal,
    pub span: Span,
}

/// A read-only view of a schema node, handed to extensions.
#[derive(Debug, Clone, Copy)]
pub struct Node<'a> {
    pub(crate) inner: &'a KdlNode,
    pub(crate) file: u32,
}

pub(crate) fn span_of(file: u32, s: miette::SourceSpan) -> Span {
    #[allow(clippy::cast_possible_truncation)]
    Span {
        file,
        offset: s.offset() as u32,
        len: s.len() as u32,
    }
}

impl<'a> Node<'a> {
    pub fn name(&self) -> &'a str {
        self.inner.name().value()
    }

    pub fn span(&self) -> Span {
        span_of(self.file, self.inner.span())
    }

    /// The span of the node's name.
    pub fn name_span(&self) -> Span {
        span_of(self.file, self.inner.name().span())
    }

    fn arg_of(&self, e: &KdlEntry) -> Arg {
        Arg {
            value: Literal::from_kdl(e.value()),
            span: span_of(self.file, e.span()),
        }
    }

    /// Positional arguments.
    pub fn args(&self) -> Vec<Arg> {
        self.inner
            .entries()
            .iter()
            .filter(|e| e.name().is_none())
            .map(|e| self.arg_of(e))
            .collect()
    }

    /// Properties in source order (`name=value`).
    pub fn props(&self) -> Vec<(&'a str, Arg)> {
        self.inner
            .entries()
            .iter()
            .filter_map(|e| e.name().map(|n| (n.value(), self.arg_of(e))))
            .collect()
    }

    /// The last property called `name`.
    pub fn prop(&self, name: &str) -> Option<Arg> {
        self.props()
            .into_iter()
            .rev()
            .find(|(n, _)| *n == name)
            .map(|(_, a)| a)
    }

    pub fn children(&self) -> Vec<Node<'a>> {
        self.inner
            .children()
            .map(|d| {
                d.nodes()
                    .iter()
                    .map(|n| Node {
                        inner: n,
                        file: self.file,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A generic JSON form: `{"name", "args", "props", "children"}`, with
    /// empty parts left out.
    pub fn to_json(&self) -> Value {
        let mut o = serde_json::Map::new();
        o.insert("name".into(), self.name().into());
        let args: Vec<Value> = self.args().iter().map(|a| a.value.to_json()).collect();
        if !args.is_empty() {
            o.insert("args".into(), args.into());
        }
        let props: serde_json::Map<String, Value> = self
            .props()
            .into_iter()
            .map(|(k, a)| (k.to_owned(), a.value.to_json()))
            .collect();
        if !props.is_empty() {
            o.insert("props".into(), props.into());
        }
        let children: Vec<Value> = self.children().iter().map(Node::to_json).collect();
        if !children.is_empty() {
            o.insert("children".into(), children.into());
        }
        Value::Object(o)
    }
}

/// A handler for domain-specific nodes and properties.
///
/// Every method has a default, so an extension implements only what it
/// claims.
pub trait Extension {
    /// The key its metadata is stored under in `ext` maps.
    fn name(&self) -> &str;

    /// Whether it handles the top-level node `node`.
    fn claims_top_level(&self, node: &str) -> bool {
        let _ = node;
        false
    }

    /// Whether it handles the child node `node` inside an item of `kind`.
    fn claims_child(&self, kind: ItemKind, node: &str) -> bool {
        let _ = (kind, node);
        false
    }

    /// Whether it handles the property `prop` on an item of `kind`.
    fn claims_property(&self, kind: ItemKind, prop: &str) -> bool {
        let _ = (kind, prop);
        false
    }

    /// Turns a claimed top-level node into metadata on the schema.
    ///
    /// # Errors
    /// A diagnostic when the node is malformed.
    fn top_level(&self, node: Node<'_>) -> Result<Value, Diagnostic> {
        Ok(node.to_json())
    }

    /// Turns a claimed child node into metadata on its item.
    ///
    /// # Errors
    /// A diagnostic when the node is malformed.
    fn child(&self, kind: ItemKind, node: Node<'_>) -> Result<Value, Diagnostic> {
        let _ = kind;
        Ok(node.to_json())
    }

    /// Turns a claimed property into metadata on its item.
    ///
    /// # Errors
    /// A diagnostic when the value is malformed.
    fn property(&self, kind: ItemKind, prop: &str, value: &Arg) -> Result<Value, Diagnostic> {
        let _ = kind;
        Ok(serde_json::json!({ prop: value.value.to_json() }))
    }

    /// Cross-checks the finished schema (e.g. that referenced keys exist).
    fn check(&self, schema: &Schema) -> Vec<Diagnostic> {
        let _ = schema;
        Vec::new()
    }
}

/// Captures nodes and properties called `name` anywhere, as generic JSON
/// ([`Node::to_json`]): metadata for plugins without writing an extension.
#[derive(Debug, Clone)]
pub struct Passthrough {
    name: String,
}

impl Passthrough {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Extension for Passthrough {
    fn name(&self) -> &str {
        &self.name
    }

    fn claims_top_level(&self, node: &str) -> bool {
        node == self.name
    }

    fn claims_child(&self, _: ItemKind, node: &str) -> bool {
        node == self.name
    }

    fn claims_property(&self, _: ItemKind, prop: &str) -> bool {
        prop == self.name
    }

    fn property(&self, _: ItemKind, _: &str, value: &Arg) -> Result<Value, Diagnostic> {
        Ok(value.value.to_json())
    }
}
