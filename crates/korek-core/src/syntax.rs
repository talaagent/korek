//! The adapter between korek and the KDL parser: the only module that
//! names the KDL tree types. Files are parsed into kdl-tree documents with
//! kdl-load (talaagent/kdl-kit), child nodes walked with kdl-nodes, and
//! kdl-span carries parse problems; everything past this module sees
//! [`Node`], [`Literal`] and korek [`Span`]s.

use kdl_load::{Entry, Value};

use korek_ir::Span;

use crate::ext::{Arg, Literal};
use alloc::sync::Arc;

use kdl_span::SourceFile;

/// A parsed file.
pub(crate) struct Document(kdl_load::Document<'static>);

impl Document {
    /// The top-level nodes; `file` is the file's index in the sources.
    pub fn nodes(&self, file: u32) -> impl Iterator<Item = Node<'_>> {
        self.0.nodes().iter().map(move |n| Node { inner: n, file })
    }
}

/// A syntax problem, with a file-local span.
pub(crate) struct Problem {
    pub message: String,
    pub label: String,
    pub span: Span,
    pub help: Option<String>,
}

/// Parses one file (`file` is its index in the sources).
pub(crate) fn parse(file: u32, name: &str, text: Arc<str>) -> Result<Document, Vec<Problem>> {
    match kdl_load::load_str(name, text) {
        Ok(loaded) => Ok(Document(loaded.into_tree().0)),
        Err(kdl_load::Error::Parse(e)) => Err(e
            .problems()
            .iter()
            .map(|p| Problem {
                message: p.message.clone(),
                label: p.label.message.clone().unwrap_or_else(|| "here".into()),
                span: span(file, p.label.span.offset, p.label.span.len),
                help: p.help.clone(),
            })
            .collect()),
        Err(e) => Err(vec![Problem {
            message: e.to_string(),
            label: "here".into(),
            span: span(file, 0, 0),
            help: None,
        }]),
    }
}

/// Parses files, one thread per file when there are several (parsing is
/// most of the work of loading).
pub(crate) fn parse_all(files: &[Arc<SourceFile>]) -> Vec<Result<Document, Vec<Problem>>> {
    #[allow(clippy::cast_possible_truncation)]
    let one = |i: usize, f: &Arc<SourceFile>| parse(i as u32, f.name(), Arc::clone(f.text_arc()));
    if files.len() < 2 {
        return files.iter().enumerate().map(|(i, f)| one(i, f)).collect();
    }
    std::thread::scope(|s| {
        let handles: Vec<_> = files
            .iter()
            .enumerate()
            .map(|(i, f)| s.spawn(move || one(i, f)))
            .collect();
        handles
            .into_iter()
            .zip(files.iter().enumerate())
            .map(|(h, (i, f))| h.join().unwrap_or_else(|_| one(i, f)))
            .collect()
    })
}

#[allow(clippy::cast_possible_truncation)]
fn span(file: u32, offset: usize, len: usize) -> Span {
    Span {
        file,
        offset: offset as u32,
        len: len as u32,
    }
}

fn literal(v: &Value<'_>) -> Literal {
    match v {
        Value::String(s) => Literal::String(s.to_string()),
        Value::Integer(i) => Literal::Int(*i),
        Value::Float(f) => Literal::Float(*f),
        Value::Bool(b) => Literal::Bool(*b),
        Value::Null => Literal::Null,
    }
}

/// A read-only view of a schema node, handed to extensions.
#[derive(Debug, Clone, Copy)]
pub struct Node<'a> {
    pub(crate) inner: &'a kdl_load::Node<'static>,
    pub(crate) file: u32,
}

impl<'a> Node<'a> {
    pub fn name(&self) -> &'a str {
        self.inner.name().value()
    }

    pub fn span(&self) -> Span {
        let s = self.inner.span();
        span(self.file, s.offset, s.len)
    }

    /// The span of the node's name.
    pub fn name_span(&self) -> Span {
        let s = self.inner.name().span();
        span(self.file, s.offset, s.len)
    }

    fn arg_of(&self, e: &Entry<'_>) -> Arg {
        Arg {
            value: literal(e.value()),
            span: span(self.file, e.span().offset, e.span().len),
        }
    }

    /// Positional arguments.
    pub fn args(&self) -> Vec<Arg> {
        self.inner.args().map(|e| self.arg_of(e)).collect()
    }

    /// Properties in source order (`name=value`).
    pub fn props(&self) -> Vec<(&'a str, Arg)> {
        self.inner
            .props()
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
        kdl_nodes::children(self.inner)
            .map(|n| Node {
                inner: n,
                file: self.file,
            })
            .collect()
    }
}
