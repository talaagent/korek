//! The runtime for korek's generated Rust: [`Validate`], validation errors
//! with paths, string format checks ([`format`](mod@format)), the base64 serde adapter
//! for `bytes` ([`base64`]), `timestamp`/`duration` parsing and the
//! `Duration` codecs for integers with a time unit ([`time`]),
//! composable serde codecs for nested fields ([`with`]), and
//! `serde_json` for `json` fields.
//!
//! Validation does not allocate on the success path: paths are built on the
//! stack ([`Path`]) and only formatted when a rule fails.

extern crate alloc;

pub mod base64;
pub mod format;
pub mod time;
pub mod with;

use core::fmt;

#[cfg(feature = "regex")]
pub use regex;
#[cfg(feature = "json")]
pub use serde_json;

/// A position in a value being validated, as a stack-allocated linked list.
#[derive(Debug, Clone, Copy)]
pub enum Path<'a> {
    Root,
    Field(&'a Path<'a>, &'static str),
    Index(&'a Path<'a>, usize),
    Key(&'a Path<'a>, &'a str),
}

impl Path<'_> {
    pub fn field(&self, name: &'static str) -> Path<'_> {
        Path::Field(self, name)
    }

    pub fn index(&self, i: usize) -> Path<'_> {
        Path::Index(self, i)
    }

    pub fn key<'b>(&'b self, k: &'b str) -> Path<'b> {
        Path::Key(self, k)
    }
}

impl fmt::Display for Path<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Path::Root => Ok(()),
            Path::Field(p, n) => {
                if matches!(p, Path::Root) {
                    write!(f, "{n}")
                } else {
                    write!(f, "{p}.{n}")
                }
            }
            Path::Index(p, i) => write!(f, "{p}[{i}]"),
            Path::Key(p, k) => write!(f, "{p}[{k:?}]"),
        }
    }
}

/// One failed rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// `field.list[2]["key"]`; empty for the value itself.
    pub path: String,
    /// The rule name (`min_len`, `email`, …).
    pub rule: &'static str,
    pub message: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{}: {}", self.path, self.message)
        }
    }
}

/// Every failed rule of a value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationErrors(pub Vec<ValidationError>);

impl ValidationErrors {
    /// Records a failure at `path`.
    pub fn push(&mut self, path: &Path<'_>, rule: &'static str, message: impl fmt::Display) {
        self.0.push(ValidationError {
            path: path.to_string(),
            rule,
            message: message.to_string(),
        });
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `Ok(())` when nothing failed.
    ///
    /// # Errors
    /// When any rule failed.
    pub fn into_result(self) -> Result<(), Self> {
        if self.0.is_empty() { Ok(()) } else { Err(self) }
    }
}

impl fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, e) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("; ")?;
            }
            write!(f, "{e}")?;
        }
        Ok(())
    }
}

impl core::error::Error for ValidationErrors {}

/// Implemented by every generated type.
pub trait Validate {
    /// Checks every rule, recording failures under `path`.
    fn validate_at(&self, path: &Path<'_>, errors: &mut ValidationErrors);

    /// Checks every rule.
    ///
    /// # Errors
    /// Every rule that failed, with paths.
    fn validate(&self) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::default();
        self.validate_at(&Path::Root, &mut errors);
        errors.into_result()
    }
}

impl<T: Validate> Validate for Option<T> {
    fn validate_at(&self, path: &Path<'_>, errors: &mut ValidationErrors) {
        if let Some(v) = self {
            v.validate_at(path, errors);
        }
    }
}

impl<T: Validate> Validate for Vec<T> {
    fn validate_at(&self, path: &Path<'_>, errors: &mut ValidationErrors) {
        for (i, v) in self.iter().enumerate() {
            v.validate_at(&path.index(i), errors);
        }
    }
}

impl<T: Validate> Validate for alloc::collections::BTreeMap<String, T> {
    fn validate_at(&self, path: &Path<'_>, errors: &mut ValidationErrors) {
        for (k, v) in self {
            v.validate_at(&path.key(k), errors);
        }
    }
}

/// Whether the items are pairwise distinct (`unique`).
pub fn unique<T: PartialEq>(items: &[T]) -> bool {
    items
        .iter()
        .enumerate()
        .all(|(i, a)| items[i + 1..].iter().all(|b| a != b))
}

/// Whether `pattern` matches somewhere in `s`, compiling it once per call
/// site (`cell` is a `static`).
#[cfg(feature = "regex")]
pub fn matches(cell: &std::sync::OnceLock<regex::Regex>, pattern: &str, s: &str) -> bool {
    cell.get_or_init(|| regex::Regex::new(pattern).unwrap_or_else(|_| never()))
        .is_match(s)
}

#[cfg(feature = "regex")]
fn never() -> regex::Regex {
    // korek checks patterns at load time; this only guards against misuse.
    regex::Regex::new("[^\\s\\S]").unwrap_or_else(|_| unreachable!())
}

/// A rule on a key, as metadata: its name and JSON operand (`""` for
/// none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleInfo {
    pub name: &'static str,
    pub operand: &'static str,
}

/// A typed key: its names, docs, default and rules.
pub struct Key<T: 'static> {
    /// The keys block, e.g. `session`.
    pub group: &'static str,
    /// The key within its block, e.g. `steer.enabled`.
    pub name: &'static str,
    /// `group.name`.
    pub path: &'static str,
    pub doc: &'static str,
    /// Builds the default value.
    pub default: Option<fn() -> T>,
    /// The default as JSON.
    pub default_json: Option<&'static str>,
    /// Never print or render the value.
    pub secret: bool,
    pub deprecated: Option<&'static str>,
    pub unit: Option<&'static str>,
    pub rules: &'static [RuleInfo],
    /// Checks the key's rules on a value.
    pub check: fn(&T, &Path<'_>, &mut ValidationErrors),
}

impl<T> Key<T> {
    /// The default value, if the key has one.
    pub fn default_value(&self) -> Option<T> {
        self.default.map(|f| f())
    }

    /// Checks the key's rules on `value`.
    ///
    /// # Errors
    /// Every rule that failed.
    pub fn validate(&self, value: &T) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::default();
        (self.check)(value, &Path::Root, &mut errors);
        errors.into_result()
    }
}

impl<T> Clone for Key<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Key<T> {}

impl<T> fmt::Debug for Key<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Key")
            .field("path", &self.path)
            .field(
                "default_json",
                &if self.secret { None } else { self.default_json },
            )
            .field("rules", &self.rules)
            .finish_non_exhaustive()
    }
}
