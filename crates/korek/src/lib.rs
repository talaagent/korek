//! korek: define types once in KDL, generate them for many languages.
//!
//! This crate is the driver: it loads and validates a schema with
//! [`korek_core`], then runs one generator plugin per output over the
//! [`korek_plugin`] protocol, and writes (or, with `check`, compares) the
//! files they return. The `korek` binary is a thin CLI over [`Driver`].
//!
//! ```no_run
//! use korek::{Driver, Output};
//!
//! let report = Driver::new()
//!     .output(Output::new("rust", "src/generated"))
//!     .output(Output::new("proto", "proto"))
//!     .run("schema".as_ref())?;
//! # Ok::<(), korek::Error>(())
//! ```
//!
//! Generators are plugins: `korek-gen-<name>` on `PATH` (or next to the
//! running binary), or a path given with [`Driver::plugin`]. Rust code that
//! wants no subprocess can call the generator crates
//! (`korek_gen_rust::generate`, …) directly with the IR from
//! [`korek_core::Loader`].

use alloc::collections::BTreeMap;
use core::fmt;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

pub use korek_core;
use korek_core::{Diagnostic, Extension, Failure, Loader, Severity, Sources};
pub use korek_plugin;
use korek_plugin::{ENCODING_ENV, Encoding, GenerateRequest, GenerateResponse};

extern crate alloc;

/// One requested output: a plugin name, a directory and the plugin's
/// options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub plugin: String,
    pub dir: PathBuf,
    pub options: BTreeMap<String, String>,
}

impl Output {
    pub fn new(plugin: impl Into<String>, dir: impl Into<PathBuf>) -> Self {
        Self {
            plugin: plugin.into(),
            dir: dir.into(),
            options: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn option(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.options.insert(k.into(), v.into());
        self
    }
}

/// What happened to one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Unchanged,
    Changed,
    New,
}

/// A file the run produced.
#[derive(Debug, Clone)]
pub struct Planned {
    pub path: PathBuf,
    pub content: String,
    pub status: Status,
    /// The plugin that produced it (`lock` for the lockfile).
    pub plugin: String,
}

/// The result of a run.
#[derive(Debug)]
#[non_exhaustive]
pub struct Report {
    pub files: Vec<Planned>,
    /// Warnings from plugins.
    pub warnings: Vec<Diagnostic>,
    /// For rendering warnings.
    pub sources: Sources,
}

impl Report {
    /// Files that differ from disk.
    pub fn stale(&self) -> impl Iterator<Item = &Planned> {
        self.files.iter().filter(|f| f.status != Status::Unchanged)
    }
}

/// Why a run failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The schema did not load.
    Schema(Failure),
    /// A plugin could not run, or broke the protocol.
    Plugin { plugin: String, message: String },
    /// A plugin reported errors.
    Generate {
        plugin: String,
        diagnostics: Vec<Diagnostic>,
        sources: Sources,
    },
    /// `check`: generated files differ from disk.
    Stale(Vec<PathBuf>),
    Io {
        path: PathBuf,
        error: std::io::Error,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Schema(e) => write!(f, "the schema has {e}"),
            Self::Plugin { plugin, message } => write!(f, "plugin `{plugin}`: {message}"),
            Self::Generate {
                plugin,
                diagnostics,
                ..
            } => {
                write!(f, "plugin `{plugin}` failed")?;
                for d in diagnostics {
                    write!(f, "\n  {}: {}", d.code, d.message)?;
                }
                Ok(())
            }
            Self::Stale(paths) => {
                write!(f, "{} generated file(s) are out of date:", paths.len())?;
                for p in paths {
                    write!(f, "\n  {}", p.display())?;
                }
                Ok(())
            }
            Self::Io { path, error } => write!(f, "{}: {error}", path.display()),
        }
    }
}

impl core::error::Error for Error {}

/// Loads a schema and runs plugins over it.
#[derive(Default)]
pub struct Driver {
    loader: Loader,
    plugins: BTreeMap<String, PathBuf>,
    outputs: Vec<Output>,
    check: bool,
}

impl Driver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an extension (see [`korek_core::Extension`]).
    #[must_use]
    pub fn extension(mut self, ext: impl Extension + 'static) -> Self {
        self.loader = self.loader.extension(ext);
        self
    }

    /// Uses `path` as the lockfile instead of `<schema>/korek.lock`.
    #[must_use]
    pub fn lock_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.loader = self.loader.lock_path(path);
        self
    }

    /// Runs plugin `name` from `path` instead of `korek-gen-<name>`.
    #[must_use]
    pub fn plugin(mut self, name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        self.plugins.insert(name.into(), path.into());
        self
    }

    #[must_use]
    pub fn output(mut self, out: Output) -> Self {
        self.outputs.push(out);
        self
    }

    /// Compare with disk instead of writing; stale files are an error.
    #[must_use]
    pub fn check(mut self, check: bool) -> Self {
        self.check = check;
        self
    }

    /// Finds the plugin binary for `name`.
    fn find(&self, name: &str) -> Result<PathBuf, Error> {
        if let Some(p) = self.plugins.get(name) {
            return Ok(p.clone());
        }
        let exe = format!("korek-gen-{name}{}", std::env::consts::EXE_SUFFIX);
        let beside = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join(&exe)));
        let on_path = std::env::var_os("PATH")
            .into_iter()
            .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
            .map(|d| d.join(&exe));
        on_path
            .chain(beside)
            .find(|p| p.is_file())
            .ok_or_else(|| Error::Plugin {
                plugin: name.to_owned(),
                message: format!("`{exe}` is not on PATH (pass --plugin {name}=<path>)"),
            })
    }

    /// Runs one plugin binary.
    fn invoke(&self, out: &Output, req: &GenerateRequest) -> Result<GenerateResponse, Error> {
        let fail = |message: String| Error::Plugin {
            plugin: out.plugin.clone(),
            message,
        };
        let path = self.find(&out.plugin)?;
        let enc = Encoding::Json;
        let input = enc.encode(req).map_err(fail)?;
        let mut child = Command::new(&path)
            .env(ENCODING_ENV, enc.name())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| fail(format!("running {}: {e}", path.display())))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(&input)
                .map_err(|e| fail(format!("writing the request: {e}")))?;
        }
        let res = child
            .wait_with_output()
            .map_err(|e| fail(format!("waiting for {}: {e}", path.display())))?;
        if !res.status.success() {
            let stderr = String::from_utf8_lossy(&res.stderr);
            return Err(fail(format!(
                "{} exited with {}: {}",
                path.display(),
                res.status,
                stderr.trim()
            )));
        }
        enc.decode(&res.stdout)
            .map_err(|e| fail(format!("bad response: {e}")))
    }

    /// Loads the schema in `dir`, runs every output's plugin, and writes the
    /// files (or, with `check`, fails if any differ from disk). The lockfile
    /// is written alongside.
    ///
    /// # Errors
    /// See [`Error`].
    pub fn run(&self, dir: &Path) -> Result<Report, Error> {
        let loaded = self.loader.load(dir).map_err(Error::Schema)?;
        let mut files = Vec::new();
        let mut warnings = Vec::new();
        for out in &self.outputs {
            let req = GenerateRequest::new(&out.plugin, loaded.schema.clone())
                .with_options(out.options.clone());
            let resp = self.invoke(out, &req)?;
            let (errors, warns): (Vec<_>, Vec<_>) = resp
                .diagnostics
                .into_iter()
                .partition(|d| d.severity == Severity::Error);
            warnings.extend(warns);
            if !errors.is_empty() {
                return Err(Error::Generate {
                    plugin: out.plugin.clone(),
                    diagnostics: errors,
                    sources: loaded.sources,
                });
            }
            for f in resp.files {
                let rel = Path::new(&f.path);
                if rel.is_absolute() || rel.components().any(|c| !matches!(c, Component::Normal(_)))
                {
                    return Err(Error::Plugin {
                        plugin: out.plugin.clone(),
                        message: format!(
                            "refusing to write outside the output directory: {}",
                            f.path
                        ),
                    });
                }
                files.push((out.plugin.clone(), out.dir.join(rel), f.content));
            }
        }
        if let Some(p) = &loaded.lock_path {
            files.push(("lock".into(), p.clone(), loaded.lock.render()));
        }
        let mut planned = Vec::new();
        for (plugin, path, content) in files {
            let status = match std::fs::read_to_string(&path) {
                Ok(old) if old == content => Status::Unchanged,
                Ok(_) => Status::Changed,
                Err(_) => Status::New,
            };
            planned.push(Planned {
                path,
                content,
                status,
                plugin,
            });
        }
        let report = Report {
            files: planned,
            warnings,
            sources: loaded.sources,
        };
        if self.check {
            let stale: Vec<PathBuf> = report.stale().map(|f| f.path.clone()).collect();
            if !stale.is_empty() {
                return Err(Error::Stale(stale));
            }
            return Ok(report);
        }
        for f in report.stale() {
            let io = |error| Error::Io {
                path: f.path.clone(),
                error,
            };
            if let Some(parent) = f.path.parent() {
                std::fs::create_dir_all(parent).map_err(io)?;
            }
            std::fs::write(&f.path, &f.content).map_err(io)?;
        }
        Ok(report)
    }
}
