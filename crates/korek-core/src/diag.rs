//! Load failures, and (with the `miette` feature) rendering diagnostics with
//! their sources.

use core::fmt;

use korek_ir::{Diagnostic, Severity};

use crate::source::Sources;

/// Loading failed: every diagnostic found (sorted by file and position),
/// with the sources to render them.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub struct Failure {
    pub diagnostics: Vec<Diagnostic>,
    pub sources: Sources,
}

impl Failure {
    pub fn new(mut diagnostics: Vec<Diagnostic>, sources: Sources) -> Self {
        sort(&mut diagnostics);
        Self {
            diagnostics,
            sources,
        }
    }

    /// Whether any diagnostic has `code`.
    pub fn has(&self, code: &str) -> bool {
        self.diagnostics.iter().any(|d| d.code == code)
    }

    /// The diagnostics as JSON objects with file, line and column, for
    /// `--message-format json`.
    pub fn to_json(&self) -> Vec<serde_json::Value> {
        self.diagnostics
            .iter()
            .map(|d| diagnostic_json(d, &self.sources))
            .collect()
    }
}

/// Sorts diagnostics by file and offset (unlocated ones last).
pub fn sort(d: &mut [Diagnostic]) {
    d.sort_by_key(|d| {
        d.span()
            .map_or((u32::MAX, u32::MAX), |s| (s.file, s.offset))
    });
}

/// One diagnostic as JSON: code, severity, message, help, url, the primary
/// location (`file`, `line`, `col`; all `null` when the diagnostic has no
/// source location, like an I/O or plugin error) and labels with file, line
/// and column.
pub fn diagnostic_json(d: &Diagnostic, sources: &Sources) -> serde_json::Value {
    let labels: Vec<serde_json::Value> = d
        .labels
        .iter()
        .map(|l| {
            let mut v = location_json(Some(l.span), sources);
            v["offset"] = l.span.offset.into();
            v["len"] = l.span.len.into();
            v["text"] = l.text.clone().into();
            v
        })
        .collect();
    let mut v = serde_json::json!({
        "code": d.code,
        "severity": match d.severity { Severity::Error => "error", Severity::Warning => "warning" },
        "message": d.message,
        "help": d.help,
        "url": d.code.starts_with("korek::").then(|| korek_ir::codes::url(&d.code)),
    });
    if let (Some(o), serde_json::Value::Object(loc)) =
        (v.as_object_mut(), location_json(d.span(), sources))
    {
        o.extend(loc);
    }
    v["labels"] = labels.into();
    v
}

/// `{file, line, col}` for a span, each `null` when unknown.
fn location_json(span: Option<korek_ir::Span>, sources: &Sources) -> serde_json::Value {
    let file = span.and_then(|s| sources.file(s)).map(|f| f.path.as_str());
    let lc = span.and_then(|s| sources.line_col(s));
    serde_json::json!({
        "file": file,
        "line": lc.map(|(l, _)| l),
        "col": lc.map(|(_, c)| c),
    })
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self.diagnostics.iter().filter(|d| d.is_error()).count();
        write!(f, "{n} error{}", if n == 1 { "" } else { "s" })?;
        for d in &self.diagnostics {
            write!(f, "\n  {}: {}", d.code, d.message)?;
        }
        Ok(())
    }
}

#[cfg(feature = "miette")]
pub use render::Rendered;

#[cfg(feature = "miette")]
mod render {
    use alloc::sync::Arc;
    use core::fmt;

    use korek_ir::{Diagnostic, Severity};
    use miette::{LabeledSpan, NamedSource, SourceSpan};

    use crate::source::Sources;

    /// A diagnostic with its source attached; implements
    /// [`miette::Diagnostic`].
    #[derive(Debug)]
    pub struct Rendered {
        diag: Diagnostic,
        url: Option<String>,
        src: Option<NamedSource<Arc<str>>>,
    }

    impl Rendered {
        pub fn new(diag: &Diagnostic, sources: &Sources) -> Self {
            let src = diag.span().and_then(|s| sources.file(s));
            Self {
                url: diag
                    .code
                    .starts_with("korek::")
                    .then(|| korek_ir::codes::url(&diag.code)),
                diag: diag.clone(),
                src: src.map(|f| NamedSource::new(&f.path, Arc::<str>::from(f.contents.as_str()))),
            }
        }
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

        fn url<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
            self.url
                .as_ref()
                .map(|u| Box::new(u) as Box<dyn fmt::Display>)
        }

        fn source_code(&self) -> Option<&dyn miette::SourceCode> {
            self.src.as_ref().map(|s| s as &dyn miette::SourceCode)
        }

        fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
            self.src.as_ref()?;
            let first = self.diag.labels.first()?.span.file;
            Some(Box::new(
                self.diag
                    .labels
                    .iter()
                    .filter(move |l| l.span.file == first)
                    .map(|l| {
                        LabeledSpan::new_with_span(
                            l.text.clone(),
                            SourceSpan::new((l.span.offset as usize).into(), l.span.len as usize),
                        )
                    }),
            ))
        }
    }

    impl super::Failure {
        /// The diagnostics paired with their sources, for printing.
        pub fn rendered(&self) -> Vec<Rendered> {
            self.diagnostics
                .iter()
                .map(|d| Rendered::new(d, &self.sources))
                .collect()
        }
    }
}
