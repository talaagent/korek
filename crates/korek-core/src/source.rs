//! Reading schema files: a directory of `.kdl` files as one ordered set of
//! sources, and line/column lookup for spans.
//!
//! Kept small and internal on purpose: it moves to the `kdl-load` crate of
//! talaagent/kdl-serde once that lands.

use std::path::{Path, PathBuf};

use korek_ir::Span;

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

impl Sources {
    /// The file a span points into.
    pub fn file(&self, span: Span) -> Option<&SourceFile> {
        self.files.get(span.file as usize)
    }

    /// 1-based line and column (in characters) of a span's start.
    pub fn line_col(&self, span: Span) -> Option<(usize, usize)> {
        let text = &self.file(span)?.contents;
        let upto = text.get(..span.offset as usize)?;
        let line = upto.matches('\n').count() + 1;
        let col = upto.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
        Some((line, col))
    }
}

/// Every `.kdl` file under `dir`, recursively, sorted by path.
///
/// # Errors
/// The path and error of the first file or directory that cannot be read.
pub fn read_dir(dir: &Path) -> Result<Vec<SourceFile>, (PathBuf, std::io::Error)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), (PathBuf, std::io::Error)> {
        let entries = std::fs::read_dir(dir).map_err(|e| (dir.to_path_buf(), e))?;
        for entry in entries {
            let path = entry.map_err(|e| (dir.to_path_buf(), e))?.path();
            if path.is_dir() {
                walk(&path, out)?;
            } else if path.extension().is_some_and(|e| e == "kdl") {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut paths = Vec::new();
    walk(dir, &mut paths)?;
    paths.sort();
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
