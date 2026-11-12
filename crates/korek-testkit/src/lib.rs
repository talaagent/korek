//! Shared validation vectors (`vectors/rules.json`): for each case, a type,
//! a `validate` block, and values every target must accept or reject. The
//! same table runs against the loader (as examples), the generated Rust
//! (this crate's tests), the generated Zod (`ts/vectors.test.ts`) and,
//! through [`run`], any third-party generator.

use core::fmt::Write as _;

use serde::Deserialize;

/// One vector.
#[derive(Debug, Clone, Deserialize)]
pub struct Case {
    pub name: String,
    /// A korek type expression.
    #[serde(rename = "type")]
    pub ty: String,
    /// The body of a `validate { … }` block.
    pub validate: String,
    pub valid: Vec<serde_json::Value>,
    pub invalid: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct File {
    cases: Vec<Case>,
}

/// The vectors file.
pub const VECTORS: &str = include_str!("../vectors/rules.json");

/// Every case.
///
/// # Panics
/// If the bundled vectors file is malformed (a test catches that).
pub fn cases() -> Vec<Case> {
    serde_json::from_str::<File>(VECTORS)
        .map_or_else(|e| panic!("vectors/rules.json: {e}"), |f| f.cases)
}

/// `min_len` → `VMinLen`: the struct holding a case's field `v`.
pub fn struct_name(case: &str) -> String {
    let pascal: String = case
        .split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                .unwrap_or_default()
        })
        .collect();
    format!("V{pascal}")
}

/// A korek schema with one struct per case (`struct VMinLen { field v … }`).
pub fn schema() -> String {
    let mut s = String::from("package \"vectors.v1\"\n");
    for c in cases() {
        let _ = write!(
            s,
            "\nstruct {} {{\n  field v {:?} {{ validate {{ {} }} }}\n}}\n",
            struct_name(&c.name),
            c.ty,
            c.validate
        );
    }
    s
}

/// A mismatch: a value accepted that should fail, or the reverse.
#[derive(Debug, Clone, PartialEq)]
pub struct Mismatch {
    pub case: String,
    pub value: serde_json::Value,
    pub expected_valid: bool,
}

/// Runs every vector through `accepts(case, value)` (whether the target
/// accepts `{"v": value}` for the case's struct) and returns mismatches.
pub fn run(mut accepts: impl FnMut(&Case, &serde_json::Value) -> bool) -> Vec<Mismatch> {
    let mut out = Vec::new();
    for c in cases() {
        for (values, expected_valid) in [(&c.valid, true), (&c.invalid, false)] {
            for v in values {
                if accepts(&c, v) != expected_valid {
                    out.push(Mismatch {
                        case: c.name.clone(),
                        value: v.clone(),
                        expected_valid,
                    });
                }
            }
        }
    }
    out
}
