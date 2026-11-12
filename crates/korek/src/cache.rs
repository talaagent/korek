//! The input-hash cache: a run whose inputs (schema files, lockfile,
//! extensions, plugin binary, options, output directory) hash the same as a
//! previous run's, and whose outputs are still on disk as written, skips
//! that plugin; when every output hits, loading is skipped too.
//!
//! One JSON entry per output under the cache directory, named by its key.
//! The cache is best effort: unreadable or corrupt entries are misses and
//! write errors are ignored.

use std::path::{Path, PathBuf};

use korek_core::{Loader, Read};
use korek_ir::Diagnostic;
use serde::{Deserialize, Serialize};

use crate::Output;

/// Bumped when the entry format or what a key covers changes.
const FORMAT: &str = "korek-cache-1";

/// Hashes everything a run reads, except the per-output parts.
pub(crate) fn inputs<'a>(
    loader: &Loader,
    files: impl IntoIterator<Item = (&'a str, &'a str)>,
    lock: Option<&str>,
) -> blake3::Hash {
    let mut h = blake3::Hasher::new();
    field(&mut h, FORMAT.as_bytes());
    field(&mut h, env!("CARGO_PKG_VERSION").as_bytes());
    // Builtins and extensions are code in this executable.
    field(
        &mut h,
        identity(std::env::current_exe().ok().as_deref()).as_bytes(),
    );
    for e in loader.extension_names() {
        field(&mut h, e.as_bytes());
    }
    for (path, contents) in files {
        field(&mut h, path.as_bytes());
        field(&mut h, contents.as_bytes());
    }
    field(&mut h, lock.unwrap_or("\0none").as_bytes());
    h.finalize()
}

/// The key of one output.
pub(crate) fn key(inputs: &blake3::Hash, out: &Output, plugin: Option<&Path>) -> String {
    let mut h = blake3::Hasher::new();
    field(&mut h, inputs.as_bytes());
    field(&mut h, out.plugin.as_bytes());
    field(&mut h, out.dir.as_os_str().as_encoded_bytes());
    field(&mut h, identity(plugin).as_bytes());
    for (k, v) in &out.options {
        field(&mut h, k.as_bytes());
        field(&mut h, v.as_bytes());
    }
    h.finalize().to_hex().to_string()
}

/// Length-prefixed, so adjacent fields cannot run together.
fn field(h: &mut blake3::Hasher, bytes: &[u8]) {
    h.update(&(bytes.len() as u64).to_le_bytes());
    h.update(bytes);
}

/// A binary's path, size and modification time (`builtin` for none).
fn identity(path: Option<&Path>) -> String {
    let Some(p) = path else {
        return "builtin".into();
    };
    let meta = std::fs::metadata(p).ok();
    let mtime = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    format!("{}:{}:{mtime}", p.display(), meta.map_or(0, |m| m.len()))
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Entry {
    /// Output files (as joined with the output directory) and their hashes.
    pub files: Vec<(PathBuf, String)>,
    /// The plugin's diagnostics (warnings; runs with errors are not cached).
    pub diagnostics: Vec<Diagnostic>,
    /// The loader's warnings, replayed when loading is skipped.
    pub load_warnings: Vec<Diagnostic>,
}

/// A hit: the entry and the files' current contents.
pub(crate) struct Hit {
    pub entry: Entry,
    pub contents: Vec<String>,
}

pub(crate) fn lookup(dir: &Path, key: &str) -> Option<Hit> {
    let bytes = std::fs::read(dir.join(format!("{key}.json"))).ok()?;
    let entry: Entry = serde_json::from_slice(&bytes).ok()?;
    let contents = entry
        .files
        .iter()
        .map(|(path, hash)| {
            let data = std::fs::read(path).ok()?;
            (blake3::hash(&data).to_hex().as_str() == hash)
                .then(|| String::from_utf8(data).ok())
                .flatten()
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Hit { entry, contents })
}

pub(crate) fn store(dir: &Path, key: &str, entry: &Entry) {
    let Ok(json) = serde_json::to_vec(entry) else {
        return;
    };
    if std::fs::create_dir_all(dir).is_ok() {
        let tmp = dir.join(format!("{key}.json.tmp"));
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, dir.join(format!("{key}.json")));
        }
    }
}

/// What [`Read`] holds, for building sources on a full hit.
pub(crate) fn sources(read: &Read) -> korek_core::Sources {
    korek_core::Sources::new(read.files.iter().chain(&read.lock).cloned())
}
