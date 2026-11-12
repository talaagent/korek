//! Diagnostics: plain data (so plugins can return them too), rendered with
//! miette against the schema's source files.

use alloc::sync::Arc;
use core::fmt;

use miette::{LabeledSpan, NamedSource, SourceSpan};
use serde::{Deserialize, Serialize};

use crate::ir::Span;

/// A source file of a schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// Path relative to the schema directory (or as given).
    pub path: String,
    pub contents: String,
}

/// The files a schema was loaded from, indexed by [`Span::file`].
#[derive(Debug, Clone, Default)]
pub struct Sources {
    pub files: Vec<SourceFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    #[default]
    Error,
    Warning,
}

/// One problem, optionally pointing at a span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Diagnostic {
    /// A stable machine-readable code, e.g. `korek::unknown_type`.
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub severity: Severity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    /// Text under the span.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn error(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
            severity: Severity::Error,
            span: None,
            label: None,
            help: None,
        }
    }

    #[must_use]
    pub fn warning(mut self) -> Self {
        self.severity = Severity::Warning;
        self
    }

    #[must_use]
    pub fn at(mut self, span: Span, label: impl Into<String>) -> Self {
        self.span = Some(span);
        self.label = Some(label.into());
        self
    }

    #[must_use]
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Pairs the diagnostic with its source for rendering.
    pub fn render(&self, sources: &Sources) -> Rendered {
        let src = self.span.and_then(|s| sources.files.get(s.file as usize));
        Rendered {
            diag: self.clone(),
            src: src.map(|f| NamedSource::new(&f.path, Arc::<str>::from(f.contents.as_str()))),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// A [`Diagnostic`] with its source attached; implements
/// [`miette::Diagnostic`].
#[derive(Debug)]
pub struct Rendered {
    diag: Diagnostic,
    src: Option<NamedSource<Arc<str>>>,
}

impl fmt::Display for Rendered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.diag.message)
    }
}

impl core::error::Error for Rendered {}

impl miette::Diagnostic for Rendered {
    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        Some(Box::new(&self.diag.code))
    }

    fn severity(&self) -> Option<miette::Severity> {
        Some(match self.diag.severity {
            Severity::Error => miette::Severity::Error,
            Severity::Warning => miette::Severity::Warning,
        })
    }

    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        self.diag
            .help
            .as_ref()
            .map(|h| Box::new(h) as Box<dyn fmt::Display>)
    }

    fn source_code(&self) -> Option<&dyn miette::SourceCode> {
        self.src.as_ref().map(|s| s as &dyn miette::SourceCode)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let span = self.diag.span?;
        self.src.as_ref()?;
        let span = SourceSpan::new((span.offset as usize).into(), span.len as usize);
        Some(Box::new(core::iter::once(LabeledSpan::new_with_span(
            self.diag.label.clone(),
            span,
        ))))
    }
}

/// Loading failed: every error found, with the sources to render them.
#[derive(Debug)]
pub struct Failure {
    pub diagnostics: Vec<Diagnostic>,
    pub sources: Sources,
}

impl Failure {
    /// The diagnostics paired with their sources, for printing.
    pub fn rendered(&self) -> Vec<Rendered> {
        self.diagnostics
            .iter()
            .map(|d| d.render(&self.sources))
            .collect()
    }

    /// Whether any diagnostic has `code`.
    pub fn has(&self, code: &str) -> bool {
        self.diagnostics.iter().any(|d| d.code == code)
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self.diagnostics.len();
        write!(f, "{n} error{}", if n == 1 { "" } else { "s" })?;
        for d in &self.diagnostics {
            write!(f, "\n  {}: {}", d.code, d.message)?;
        }
        Ok(())
    }
}

impl core::error::Error for Failure {}
