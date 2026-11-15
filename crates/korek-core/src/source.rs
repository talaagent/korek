//! Reading schema files: a directory of `.kdl` files as one ordered set of
//! sources, and line/column lookup for spans. Listing and the source map
//! come from `kdl-load` and `kdl-span` (talaagent/kdl-kit).

use alloc::sync::Arc;
use std::path::{Path, PathBuf};

use korek_ir::Span;

/// A source file of a schema, as read (the input to
/// [`Loader::load_sources`](crate::Loader::load_sources)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// Path relative to the schema directory (or as given).
    pub path: String,
    pub contents: String,
}

/// The files a schema was loaded from, indexed by [`Span::file`].
#[derive(Debug, Clone, Default)]
pub struct Sources {
    map: kdl_span::SourceMap,
}

impl Sources {
    /// Sources of `files`, in order.
    pub fn new(files: impl IntoIterator<Item = SourceFile>) -> Self {
        let mut s = Self::default();
        for f in files {
            s.push(f);
        }
        s
    }

    /// Adds a file and returns its index (its spans' [`Span::file`]).
    pub fn push(&mut self, file: SourceFile) -> u32 {
        let id = self.map.add(file.path, file.contents);
        u32::try_from(id.index()).unwrap_or(u32::MAX)
    }

    /// The files, in order (`name()` is the path, `text()` the contents).
    pub fn files(&self) -> &[Arc<kdl_span::SourceFile>] {
        self.map.files()
    }

    /// The underlying `kdl-span` source map.
    pub fn map(&self) -> &kdl_span::SourceMap {
        &self.map
    }

    /// The file a span points into.
    pub fn file(&self, span: Span) -> Option<&Arc<kdl_span::SourceFile>> {
        self.map.files().get(span.file as usize)
    }

    /// 1-based line and column (in characters) of a span's start.
    pub fn line_col(&self, span: Span) -> Option<(usize, usize)> {
        let file = self.file(span)?;
        let offset = span.offset as usize;
        (offset <= file.text().len()).then(|| file.line_col(offset))
    }
}

/// Every `.kdl` file under `dir`, recursively, sorted by path, with paths
/// relative to `dir`.
///
/// # Errors
/// The path and error of the first file or directory that cannot be read.
pub fn read_dir(dir: &Path) -> Result<Vec<SourceFile>, (PathBuf, std::io::Error)> {
    let paths = kdl_load::kdl_files(dir, true).map_err(|e| match e {
        kdl_load::Error::Io { path, source } => (path, source),
        other => (dir.to_path_buf(), std::io::Error::other(other.to_string())),
    })?;
    paths
        .into_iter()
        .map(|p| {
            let contents = std::fs::read_to_string(&p).map_err(|e| (p.clone(), e))?;
            let rel = p.strip_prefix(dir).unwrap_or(&p);
            Ok(SourceFile {
                path: rel.to_string_lossy().replace('\\', "/"),
                contents,
            })
        })
        .collect()
}
