//! korek's front end: load a directory of KDL schema files, lower them to
//! the [IR](ir), validate it and assign stable Protobuf numbers.
//!
//! ```no_run
//! let schema = korek_core::load("schema".as_ref())?;
//! for t in &schema.types {
//!     println!("{}", t.name);
//! }
//! # Ok::<(), korek_core::Failure>(())
//! ```
//!
//! Domain-specific nodes are handled by [`Extension`]s registered on a
//! [`Loader`]; their metadata lands in the IR's `ext` maps.

extern crate alloc;

pub mod diag;
pub mod ext;
pub mod ir;
pub mod lock;
pub mod naming;
mod parse;
pub mod types;
mod validate;
pub mod value;

use std::path::{Path, PathBuf};

pub use diag::{Diagnostic, Failure, Severity, SourceFile, Sources};
pub use ext::{Extension, ItemKind, Passthrough};
pub use ir::Schema;
pub use lock::{LOCK_FILE, Lock};

/// Where the lockfile comes from.
#[derive(Debug, Clone, Default)]
enum LockAt {
    /// `korek.lock` in the schema directory.
    #[default]
    InDir,
    Path(PathBuf),
}

/// Loads schemas, with extensions and a lockfile location.
#[derive(Default)]
pub struct Loader {
    exts: Vec<Box<dyn Extension>>,
    lock: LockAt,
}

/// A loaded schema with what it was loaded from.
#[derive(Debug)]
#[non_exhaustive]
pub struct Loaded {
    pub schema: Schema,
    pub sources: Sources,
    /// The lock after numbering: write it back with [`Lock::render`].
    pub lock: Lock,
    /// Where the lockfile lives (when loaded from a directory).
    pub lock_path: Option<PathBuf>,
}

/// Loads every `.kdl` file under `dir` with no extensions.
///
/// # Errors
/// Every problem found, with spans.
pub fn load(dir: &Path) -> Result<Schema, Failure> {
    Loader::new().load(dir).map(|l| l.schema)
}

fn io_failure(path: &Path, e: &std::io::Error) -> Failure {
    Failure {
        diagnostics: vec![Diagnostic::error(
            "korek::io",
            format!("{}: {e}", path.display()),
        )],
        sources: Sources::default(),
    }
}

fn kdl_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            kdl_files(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "kdl") {
            out.push(path);
        }
    }
    Ok(())
}

impl Loader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an extension.
    #[must_use]
    pub fn extension(mut self, ext: impl Extension + 'static) -> Self {
        self.exts.push(Box::new(ext));
        self
    }

    /// Reads and writes the lockfile at `path` instead of `<dir>/korek.lock`.
    #[must_use]
    pub fn lock_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.lock = LockAt::Path(path.into());
        self
    }

    /// Loads every `.kdl` file under `dir` (recursively, in path order) as
    /// one schema.
    ///
    /// # Errors
    /// Every problem found, with spans.
    pub fn load(&self, dir: &Path) -> Result<Loaded, Failure> {
        let mut paths = Vec::new();
        kdl_files(dir, &mut paths).map_err(|e| io_failure(dir, &e))?;
        paths.sort();
        let mut files = Vec::new();
        for p in &paths {
            let contents = std::fs::read_to_string(p).map_err(|e| io_failure(p, &e))?;
            let rel = p.strip_prefix(dir).unwrap_or(p);
            files.push(SourceFile {
                path: rel.to_string_lossy().replace('\\', "/"),
                contents,
            });
        }
        let lock_path = match &self.lock {
            LockAt::InDir => dir.join(LOCK_FILE),
            LockAt::Path(p) => p.clone(),
        };
        let lock = match std::fs::read_to_string(&lock_path) {
            Ok(contents) => Some(SourceFile {
                path: lock_path.to_string_lossy().into_owned(),
                contents,
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(io_failure(&lock_path, &e)),
        };
        let mut loaded = self.load_sources(files, lock)?;
        loaded.lock_path = Some(lock_path);
        Ok(loaded)
    }

    /// Loads in-memory files as one schema, numbering against `lock`.
    ///
    /// # Errors
    /// Every problem found, with spans.
    pub fn load_sources(
        &self,
        files: Vec<SourceFile>,
        lock: Option<SourceFile>,
    ) -> Result<Loaded, Failure> {
        let mut p = parse::Parser::new(&self.exts);
        for f in &files {
            p.file(f);
        }
        let mut sources = Sources { files };
        let fail = |diagnostics, sources| Err(Failure { diagnostics, sources });
        if !p.diags.is_empty() {
            return fail(p.diags, sources);
        }
        let mut diags = Vec::new();
        let old = match lock {
            Some(l) => {
                #[allow(clippy::cast_possible_truncation)]
                let id = sources.files.len() as u32;
                let parsed = Lock::parse(&l, id);
                sources.files.push(l);
                match parsed {
                    Ok(lock) => lock,
                    Err(d) => return fail(d, sources),
                }
            }
            None => Lock::default(),
        };
        let mut v = validate::Validator {
            schema: &p.schema,
            aux: &p.aux,
            diags: Vec::new(),
            resolved: Vec::new(),
        };
        v.run();
        let resolved = core::mem::take(&mut v.resolved);
        diags.append(&mut v.diags);
        let mut schema = p.schema;
        let new_lock = lock::assign(&mut schema, &p.aux.id, &old, &mut diags);
        for g in &mut schema.keys {
            for k in &mut g.keys {
                k.default = resolved
                    .iter()
                    .find(|(s, _)| *s == k.span)
                    .map(|(_, j)| j.clone());
            }
        }
        if diags.is_empty() {
            for x in &self.exts {
                diags.extend(x.check(&schema));
            }
        }
        if diags.iter().any(|d| d.severity == Severity::Error) {
            return fail(diags, sources);
        }
        Ok(Loaded {
            schema,
            sources,
            lock: new_lock,
            lock_path: None,
        })
    }
}
