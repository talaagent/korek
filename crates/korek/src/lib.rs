//! korek: define types once in KDL, generate them for many languages.
//!
//! This crate is the library facade: it re-exports the parser
//! ([`korek_core`]), the IR ([`ir`]), the plugin protocol ([`plugin`]), the
//! runtime for generated Rust ([`runtime`]) and, with the default
//! `generators` feature, the first-party generators ([`generators`]); and it
//! has the [`Driver`] the `korek` CLI runs.
//!
//! ```no_run
//! use korek::{Driver, Output};
//!
//! let report = Driver::new()
//!     .with_builtins()
//!     .output(Output::new("rust", "src/generated"))
//!     .output(Output::new("proto", "proto"))
//!     .run("schema".as_ref())?;
//! # Ok::<(), korek::Error>(())
//! ```

use alloc::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

pub use korek_core::{self, Extension, Loader, Passthrough};
pub use korek_ir as ir;
pub use korek_plugin as plugin;
pub use korek_runtime as runtime;
pub use korek_runtime::{Validate, ValidationErrors};

use korek_core::{Sources, codes};
use korek_ir::{Diagnostic, GenerateRequest, GenerateResponse, Severity};
use korek_plugin::{ENCODING_ENV, Encoding};

mod cache;

extern crate alloc;

/// The first-party generators, callable in-process.
#[cfg(feature = "generators")]
pub mod generators {
    pub use korek_gen_docs as docs;
    pub use korek_gen_jsonschema as jsonschema;
    pub use korek_gen_proto as proto;
    pub use korek_gen_rust as rust;
    pub use korek_gen_ts as ts;
    pub use korek_gen_zod as zod;
}

/// The binary a resolved plugin runs (`None` for a builtin).
fn binary(how: &Result<Result<PathBuf, Builtin>, Error>) -> Option<&Path> {
    match how {
        Ok(Ok(p)) => Some(p),
        _ => None,
    }
}

/// An in-process generator: the same request and response as a plugin.
pub type Builtin = fn(&GenerateRequest) -> Result<GenerateResponse, Diagnostic>;

/// One requested output: a plugin name, a directory and options.
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

/// How a plugin ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Builtin,
    Subprocess,
    /// Skipped: its inputs and outputs are as the cache recorded.
    Cached,
}

/// The result of a run.
#[derive(Debug)]
#[non_exhaustive]
pub struct Report {
    pub files: Vec<Planned>,
    pub warnings: Vec<Diagnostic>,
    /// How each plugin ran.
    pub plugins: Vec<(String, Via)>,
    pub sources: Sources,
}

impl Report {
    /// Files that differ from disk.
    pub fn stale(&self) -> impl Iterator<Item = &Planned> {
        self.files.iter().filter(|f| f.status != Status::Unchanged)
    }
}

/// A failed run: every diagnostic, with the sources to render them.
#[derive(Debug, thiserror::Error)]
#[error("{}", summary(.diagnostics))]
#[non_exhaustive]
pub struct Error {
    pub diagnostics: Vec<Diagnostic>,
    pub sources: Sources,
}

fn summary(d: &[Diagnostic]) -> String {
    d.iter()
        .map(|d| format!("{}: {}", d.code, d.message))
        .collect::<Vec<_>>()
        .join("\n")
}

impl From<korek_core::Failure> for Error {
    fn from(f: korek_core::Failure) -> Self {
        Self {
            diagnostics: f.diagnostics,
            sources: f.sources,
        }
    }
}

impl Error {
    fn one(code: &str, message: impl Into<String>) -> Self {
        Self {
            diagnostics: vec![Diagnostic::error(code, message)],
            sources: Sources::default(),
        }
    }

    /// Whether any diagnostic has `code`.
    pub fn has(&self, code: &str) -> bool {
        self.diagnostics.iter().any(|d| d.code == code)
    }
}

/// Loads a schema and runs plugins over it.
#[derive(Default)]
pub struct Driver {
    loader: Loader,
    plugins: BTreeMap<String, PathBuf>,
    builtins: BTreeMap<String, Builtin>,
    outputs: Vec<Output>,
    check: bool,
    strict: bool,
    no_path_lookup: bool,
    cache: Option<PathBuf>,
}

impl Driver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the first-party generators as in-process builtins.
    #[cfg(feature = "generators")]
    #[must_use]
    pub fn with_builtins(self) -> Self {
        self.builtin("rust", korek_gen_rust::plugin)
            .builtin("proto", korek_gen_proto::plugin)
            .builtin("zod", korek_gen_zod::plugin)
            .builtin("ts", korek_gen_ts::plugin)
            .builtin("jsonschema", korek_gen_jsonschema::plugin)
            .builtin("docs", korek_gen_docs::plugin)
    }

    /// Runs `name` in-process unless a binary overrides it.
    #[must_use]
    pub fn builtin(mut self, name: impl Into<String>, f: Builtin) -> Self {
        self.builtins.insert(name.into(), f);
        self
    }

    #[must_use]
    pub fn extension(mut self, ext: impl Extension + Send + Sync + 'static) -> Self {
        self.loader = self.loader.extension(ext);
        self
    }

    #[must_use]
    pub fn lock_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.loader = self.loader.lock_path(path);
        self
    }

    /// Runs plugin `name` from `path` (overrides builtins).
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

    /// Treat `korek::rule_unsupported` warnings as errors.
    #[must_use]
    pub fn strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    /// Don't look for `korek-gen-<name>` on `PATH` (builtins win).
    #[must_use]
    pub fn no_path_lookup(mut self, yes: bool) -> Self {
        self.no_path_lookup = yes;
        self
    }

    /// Caches runs under `dir` (see [`Driver::run`]); off by default.
    #[must_use]
    pub fn cache(mut self, dir: Option<PathBuf>) -> Self {
        self.cache = dir;
        self
    }

    /// How plugin `name` would run: a binary, or a builtin.
    fn resolve(&self, name: &str) -> Result<Result<PathBuf, Builtin>, Error> {
        if let Some(p) = self.plugins.get(name) {
            return Ok(Ok(p.clone()));
        }
        let exe = format!("korek-gen-{name}{}", std::env::consts::EXE_SUFFIX);
        if !self.no_path_lookup {
            let on_path = std::env::var_os("PATH")
                .into_iter()
                .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
                .map(|d| d.join(&exe))
                .find(|p| p.is_file());
            if let Some(p) = on_path {
                return Ok(Ok(p));
            }
        }
        if let Some(f) = self.builtins.get(name) {
            return Ok(Err(*f));
        }
        let beside = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join(&exe)));
        beside.filter(|p| p.is_file()).map(Ok).ok_or_else(|| {
            let mut e = Error::one(
                codes::PLUGIN_NOT_FOUND,
                format!("no plugin `{name}`: `{exe}` is not on PATH"),
            );
            e.diagnostics[0].help = Some(format!("install it, or pass --plugin {name}=<path>"));
            e
        })
    }

    fn invoke(path: &Path, req: &GenerateRequest) -> Result<GenerateResponse, Error> {
        let fail =
            |code: &str, m: String| Error::one(code, format!("plugin `{}`: {m}", req.plugin));
        let enc = Encoding::Json;
        let input = enc
            .encode(req)
            .map_err(|e| fail(codes::PLUGIN_PROTOCOL, e))?;
        let mut child = Command::new(path)
            .env(ENCODING_ENV, enc.name())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                fail(
                    codes::PLUGIN_FAILED,
                    format!("running {}: {e}", path.display()),
                )
            })?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(&input)
                .map_err(|e| fail(codes::PLUGIN_FAILED, format!("writing the request: {e}")))?;
        }
        let res = child.wait_with_output().map_err(|e| {
            fail(
                codes::PLUGIN_FAILED,
                format!("waiting for {}: {e}", path.display()),
            )
        })?;
        if !res.status.success() {
            let stderr = String::from_utf8_lossy(&res.stderr);
            return Err(fail(
                codes::PLUGIN_FAILED,
                format!(
                    "{} exited with {}: {}",
                    path.display(),
                    res.status,
                    stderr.trim()
                ),
            ));
        }
        enc.decode(&res.stdout)
            .map_err(|e| fail(codes::PLUGIN_PROTOCOL, format!("bad response: {e}")))
    }

    /// Runs one output's plugin.
    fn generate(
        out: &Output,
        how: Result<&Path, Builtin>,
        schema: &korek_ir::Schema,
    ) -> Result<(GenerateResponse, Via), Error> {
        let req =
            GenerateRequest::new(&out.plugin, schema.clone()).with_options(out.options.clone());
        match how {
            Ok(path) => Self::invoke(path, &req).map(|r| (r, Via::Subprocess)),
            Err(f) => Ok((
                f(&req).unwrap_or_else(GenerateResponse::error),
                Via::Builtin,
            )),
        }
    }

    /// Loads the schema in `dir`, runs every output's plugin (in parallel),
    /// and writes the files whose content changed (or, with `check`, fails
    /// if any differ). The lockfile is written alongside.
    ///
    /// With a [`cache`](Driver::cache), an output whose inputs hash as a
    /// previous run's and whose files are unchanged on disk is not
    /// regenerated; when all are, the schema is not even loaded.
    ///
    /// # Errors
    /// Every diagnostic, from loading, plugins or `check`.
    #[allow(clippy::too_many_lines)]
    pub fn run(&self, dir: &Path) -> Result<Report, Error> {
        let read = self.loader.read(dir)?;
        let how: Vec<Result<Result<PathBuf, Builtin>, Error>> = self
            .outputs
            .iter()
            .map(|o| self.resolve(&o.plugin))
            .collect();
        let inputs = self.cache.as_ref().map(|_| {
            cache::inputs(
                &self.loader,
                read.files
                    .iter()
                    .map(|f| (f.path.as_str(), f.contents.as_str())),
                read.lock.as_ref().map(|l| l.contents.as_str()),
            )
        });
        let mut hits: Vec<Option<cache::Hit>> = self
            .outputs
            .iter()
            .zip(&how)
            .map(|(o, h)| {
                let (Some(c), Some(i), Ok(_)) = (&self.cache, &inputs, h) else {
                    return None;
                };
                cache::lookup(c, &cache::key(i, o, binary(h)))
            })
            .collect();
        let n_files = read.files.len();
        let (loaded, sources, mut warnings) =
            if !self.outputs.is_empty() && hits.iter().all(Option::is_some) {
                let warnings = hits
                    .first()
                    .and_then(Option::as_ref)
                    .map(|h| h.entry.load_warnings.clone())
                    .unwrap_or_default();
                (None, cache::sources(&read), warnings)
            } else {
                let mut l = self.loader.load_sources(read.files, read.lock)?;
                l.lock_path = Some(read.lock_path.clone());
                let w = l.warnings.clone();
                (Some(l), Sources::default(), w)
            };
        let results: Vec<Option<Result<(GenerateResponse, Via), Error>>> =
            std::thread::scope(|s| {
                let handles: Vec<_> = self
                    .outputs
                    .iter()
                    .zip(&how)
                    .zip(&hits)
                    .map(|((o, h), hit)| {
                        let schema = loaded.as_ref().map(|l| &l.schema);
                        (hit.is_none()).then(|| {
                            s.spawn(move || match (h, schema) {
                                (Err(e), _) => Err(Error {
                                    diagnostics: e.diagnostics.clone(),
                                    sources: Sources::default(),
                                }),
                                (Ok(how), Some(schema)) => {
                                    Self::generate(o, how.as_deref().map_err(|f| *f), schema)
                                }
                                (Ok(_), None) => Err(Error::one(codes::PLUGIN_FAILED, "no schema")),
                            })
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|h| {
                        h.map(|h| {
                            h.join().unwrap_or_else(|_| {
                                Err(Error::one(codes::PLUGIN_FAILED, "a plugin thread panicked"))
                            })
                        })
                    })
                    .collect()
            });
        let mut files = Vec::new();
        let mut errors = Vec::new();
        let mut plugins = Vec::new();
        // Fresh outputs to record: (index, files' range, diagnostics).
        let mut fresh = Vec::new();
        for (i, (out, res)) in self.outputs.iter().zip(results).enumerate() {
            let (diagnostics, start) = (errors.len(), files.len());
            let resp = match (res, hits[i].take()) {
                (_, Some(hit)) => {
                    plugins.push((out.plugin.clone(), Via::Cached));
                    self.diagnostics(hit.entry.diagnostics, &mut warnings, &mut errors);
                    for ((path, _), content) in hit.entry.files.into_iter().zip(hit.contents) {
                        files.push((out.plugin.clone(), path, content));
                    }
                    continue;
                }
                (Some(Ok((resp, via))), None) => {
                    plugins.push((out.plugin.clone(), via));
                    resp
                }
                (Some(Err(e)), None) => {
                    errors.extend(e.diagnostics);
                    continue;
                }
                (None, None) => continue,
            };
            let ds = resp.diagnostics.clone();
            self.diagnostics(resp.diagnostics, &mut warnings, &mut errors);
            for f in resp.files {
                let rel = Path::new(&f.path);
                if rel.is_absolute() || rel.components().any(|c| !matches!(c, Component::Normal(_)))
                {
                    errors.push(Diagnostic::error(
                        codes::PLUGIN_PROTOCOL,
                        format!(
                            "plugin `{}` tried to write outside its output directory: {}",
                            out.plugin, f.path
                        ),
                    ));
                    continue;
                }
                files.push((out.plugin.clone(), out.dir.join(rel), f.content));
            }
            if errors.len() == diagnostics {
                fresh.push((i, start..files.len(), ds));
            }
        }
        let sources = loaded.as_ref().map_or(sources, |l| l.sources.clone());
        if !errors.is_empty() {
            korek_core::diag::sort(&mut errors);
            return Err(Error {
                diagnostics: errors,
                sources,
            });
        }
        let lock = match &loaded {
            Some(l) => l.lock_path.clone().map(|p| (p, l.lock.render())),
            None => sources
                .files()
                .get(n_files)
                .map(|f| (read.lock_path.clone(), f.text().to_owned())),
        };
        if let Some((p, text)) = &lock {
            files.push(("lock".into(), p.clone(), text.clone()));
        }
        let files = files
            .into_iter()
            .map(|(plugin, path, content)| {
                let status = match std::fs::read(&path) {
                    Ok(old) if old == content.as_bytes() => Status::Unchanged,
                    Ok(_) => Status::Changed,
                    Err(_) => Status::New,
                };
                Planned {
                    path,
                    content,
                    status,
                    plugin,
                }
            })
            .collect();
        let report = Report {
            files,
            warnings,
            plugins,
            sources,
        };
        self.settle(&report)?;
        if let (Some(dir), Some(l)) = (&self.cache, &loaded) {
            self.remember(
                dir,
                l,
                n_files,
                lock.as_ref().map(|(_, t)| t.as_str()),
                &report,
                fresh,
                &how,
            );
        }
        Ok(report)
    }

    /// With `check`, fails on stale files; else writes them.
    fn settle(&self, report: &Report) -> Result<(), Error> {
        if self.check {
            let stale: Vec<Diagnostic> = report
                .stale()
                .map(|f| {
                    Diagnostic::error(
                        codes::STALE_OUTPUT,
                        format!("{} is out of date", f.path.display()),
                    )
                    .help("run `korek gen` without --check")
                })
                .collect();
            if !stale.is_empty() {
                return Err(Error {
                    diagnostics: stale,
                    sources: report.sources.clone(),
                });
            }
        } else {
            for f in report.stale() {
                let io =
                    |e: std::io::Error| Error::one(codes::IO, format!("{}: {e}", f.path.display()));
                if let Some(parent) = f.path.parent() {
                    std::fs::create_dir_all(parent).map_err(io)?;
                }
                std::fs::write(&f.path, &f.content).map_err(io)?;
            }
        }
        Ok(())
    }

    /// Records the fresh outputs in the cache, keyed by the inputs as they
    /// are now on disk (the lock as written).
    #[allow(clippy::too_many_arguments)]
    fn remember(
        &self,
        dir: &Path,
        l: &korek_core::Loaded,
        n_files: usize,
        lock: Option<&str>,
        report: &Report,
        fresh: Vec<(usize, core::ops::Range<usize>, Vec<Diagnostic>)>,
        how: &[Result<Result<PathBuf, Builtin>, Error>],
    ) {
        let inputs = cache::inputs(
            &self.loader,
            l.sources.files()[..n_files]
                .iter()
                .map(|f| (f.name(), f.text())),
            lock,
        );
        for (i, range, diagnostics) in fresh {
            let out = &self.outputs[i];
            let entry = cache::Entry {
                files: report.files[range]
                    .iter()
                    .map(|f| {
                        (
                            f.path.clone(),
                            blake3::hash(f.content.as_bytes()).to_hex().to_string(),
                        )
                    })
                    .collect(),
                diagnostics,
                load_warnings: l.warnings.clone(),
            };
            cache::store(dir, &cache::key(&inputs, out, binary(&how[i])), &entry);
        }
    }

    /// Sorts a plugin's diagnostics into warnings and errors (`--strict`
    /// makes unsupported rules errors).
    fn diagnostics(
        &self,
        ds: Vec<Diagnostic>,
        warnings: &mut Vec<Diagnostic>,
        errors: &mut Vec<Diagnostic>,
    ) {
        for mut d in ds {
            if self.strict && d.code == codes::RULE_UNSUPPORTED {
                d.severity = Severity::Error;
            }
            if d.is_error() {
                errors.push(d);
            } else {
                warnings.push(d);
            }
        }
    }
}
