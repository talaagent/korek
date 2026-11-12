//! korek's front end: load a directory of KDL schema files, lower them to
//! the IR ([`korek_ir`]), validate it and assign stable Protobuf numbers.
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
//! [`Loader`]; their metadata lands in the IR's `ext` maps. With the
//! `miette` feature, [`Failure::rendered`] gives `miette` diagnostics.

extern crate alloc;

mod check;
pub mod diag;
pub mod ext;
pub mod lock;
mod parse;
pub mod source;
mod syntax;
pub mod types;
mod validate;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use diag::Failure;
pub use ext::{Extension, ItemKind, Node, Passthrough};
pub use korek_ir as ir;
pub use korek_ir::{Diagnostic, Schema, Severity, codes};
pub use lock::{LOCK_FILE, Lock};
pub use source::{SourceFile, Sources};

/// Where the lockfile comes from.
#[derive(Debug, Clone, Default)]
enum LockAt {
    #[default]
    InDir,
    Path(PathBuf),
}

/// Loads schemas, with extensions and a lockfile location.
#[derive(Default)]
pub struct Loader {
    exts: Vec<Box<dyn Extension + Send + Sync>>,
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
    /// Warnings.
    pub warnings: Vec<Diagnostic>,
}

/// A schema directory's files, read but not loaded ([`Loader::read`]).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Read {
    pub files: Vec<SourceFile>,
    /// The lockfile, if it exists.
    pub lock: Option<SourceFile>,
    pub lock_path: PathBuf,
}

/// Loads every `.kdl` file under `dir` with no extensions.
///
/// # Errors
/// Every problem found, with spans.
pub fn load(dir: &Path) -> Result<Schema, Failure> {
    Loader::new().load(dir).map(|l| l.schema)
}

fn io_failure(path: &Path, e: &std::io::Error) -> Failure {
    Failure::new(
        vec![Diagnostic::error(
            codes::IO,
            format!("{}: {e}", path.display()),
        )],
        Sources::default(),
    )
}

impl Loader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an extension.
    #[must_use]
    pub fn extension(mut self, ext: impl Extension + Send + Sync + 'static) -> Self {
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
        let read = self.read(dir)?;
        let mut loaded = self.load_sources(read.files, read.lock)?;
        loaded.lock_path = Some(read.lock_path);
        Ok(loaded)
    }

    /// Reads the schema files and the lockfile of `dir` without loading
    /// them (what [`Loader::load`] starts with; a cache can hash them).
    ///
    /// # Errors
    /// A file or directory that cannot be read.
    pub fn read(&self, dir: &Path) -> Result<Read, Failure> {
        let files = source::read_dir(dir).map_err(|(p, e)| io_failure(&p, &e))?;
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
        Ok(Read {
            files,
            lock,
            lock_path,
        })
    }

    /// The names of the registered extensions, in order.
    pub fn extension_names(&self) -> impl Iterator<Item = &str> {
        self.exts.iter().map(|e| e.name())
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
        let exts: Vec<&dyn Extension> = self.exts.iter().map(|e| &**e as &dyn Extension).collect();
        let mut sources = Sources::new(files);
        let mut p = parse::Parser::new(&exts);
        p.files(&sources);
        if !p.diags.is_empty() {
            return Err(Failure::new(p.diags, sources));
        }
        let old = match lock {
            Some(l) => {
                let parsed = Lock::parse(&l, sources.files().len().try_into().unwrap_or(u32::MAX));
                sources.push(l);
                match parsed {
                    Ok(lock) => lock,
                    Err(d) => return Err(Failure::new(d, sources)),
                }
            }
            None => Lock::default(),
        };
        let mut v = validate::Validator {
            schema: &p.schema,
            index: p.schema.index(),
            aux: &p.aux,
            diags: Vec::new(),
            resolved: Vec::new(),
            examples: Vec::new(),
        };
        v.run();
        let (resolved, examples, mut diags) = (v.resolved, v.examples, v.diags);
        let mut schema = p.schema;
        let new_lock = lock::assign(&mut schema, &p.aux.id, &old, &mut diags);
        finish(&mut schema, resolved, examples);
        if diags.is_empty() {
            for x in &self.exts {
                diags.extend(x.check(&schema));
            }
        }
        if diags.iter().any(Diagnostic::is_error) {
            return Err(Failure::new(diags, sources));
        }
        diag::sort(&mut diags);
        Ok(Loaded {
            schema,
            sources,
            lock: new_lock,
            lock_path: None,
            warnings: diags,
        })
    }
}

/// Moves resolved defaults and examples into the IR.
fn finish(
    schema: &mut Schema,
    resolved: Vec<(ir::Span, ir::Json)>,
    examples: Vec<(ir::Span, Vec<ir::Json>)>,
) {
    let mut resolved: HashMap<ir::Span, ir::Json> = resolved.into_iter().collect();
    let mut examples: HashMap<ir::Span, Vec<ir::Json>> = examples.into_iter().collect();
    let mut examples_of = |s: ir::Span| examples.remove(&s).unwrap_or_default();
    for t in &mut schema.types {
        t.attrs.examples = examples_of(t.span);
        if let ir::TypeKind::Struct(s) = &mut t.kind {
            for f in &mut s.fields {
                f.default = resolved.remove(&f.span);
                f.attrs.examples = examples_of(f.span);
            }
        }
    }
    for g in &mut schema.keys {
        for k in &mut g.keys {
            k.default = resolved.remove(&k.span);
            k.attrs.examples = examples_of(k.span);
        }
    }
}
