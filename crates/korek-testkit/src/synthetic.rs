//! A large synthetic schema for benchmarks: `modules` groups of five types
//! (an enum, two structs, a tagged and an untagged union) whose unions
//! chain into the previous group's (20 deep), heavy validation on every
//! field, keys blocks of ten keys, and services.

use core::fmt::Write as _;

/// How big to make it.
#[derive(Debug, Clone, Copy)]
pub struct Size {
    /// Groups of five types.
    pub modules: usize,
    /// Keys blocks of ten keys.
    pub key_groups: usize,
    /// Services of five RPCs.
    pub services: usize,
}

impl Size {
    /// About 2000 types, 500 keys and 100 RPCs.
    pub const LARGE: Self = Self {
        modules: 400,
        key_groups: 50,
        services: 20,
    };
}

/// Union chains restart every `DEPTH` groups.
const DEPTH: usize = 20;

/// Groups per types file in [`files`].
const PER_FILE: usize = 25;

/// The schema as one file's text.
pub fn schema(size: Size) -> String {
    files(size).into_iter().map(|(_, text)| text).collect()
}

/// The schema as files (`types_00.kdl` with 25 groups each, `keys.kdl`,
/// `services.kdl`), as a real schema directory would split it.
pub fn files(size: Size) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (n, start) in (0..size.modules).step_by(PER_FILE).enumerate() {
        let mut s = String::with_capacity(PER_FILE * 2400);
        if n == 0 {
            s.push_str("package \"bench.v1\"\n");
        }
        for i in start..(start + PER_FILE).min(size.modules) {
            module(&mut s, i);
        }
        out.push((format!("types_{n:02}.kdl"), s));
    }
    let mut s = String::with_capacity(size.key_groups * 1400);
    for g in 0..size.key_groups {
        keys(&mut s, g, size.modules);
    }
    out.push(("keys.kdl".into(), s));
    let mut s = String::new();
    for v in 0..size.services {
        service(&mut s, v, size.modules);
    }
    out.push(("services.kdl".into(), s));
    out
}

fn module(s: &mut String, i: usize) {
    let _ = write!(
        s,
        r#"
enum E{i} {{
  doc "Enum {i}."
  variant alpha {{ doc "First." }}
  variant beta
  variant gamma
  variant delta
}}

struct S{i} {{
  doc "Struct {i}."
  field name "string" {{ validate {{ min_len 1; max_len 200; pattern "^[a-z][a-z0-9_]*$" }}; example "abc" }}
  field email "string" optional=#true {{ validate {{ email; max_len 254 }} }}
  field url "string" optional=#true {{ validate {{ url; prefix "https://" message="must use HTTPS" }} }}
  field port "u32" default=8080 {{ validate {{ gte 1; lte 65535 }} }}
  field ratio "f64" default=0.5 {{ validate {{ gte 0.0; lte 1.0; finite }} }}
  field tags "list<string>" default="[]" {{ validate {{ max_items 32; unique; items {{ min_len 1; max_len 64; pattern "^[a-z-]+$" }} }} }}
  field labels "map<string,i64>" default="{{}}" {{ validate {{ max_pairs 64; keys {{ max_len 63 }}; values {{ gte 0 }} }} }}
  field blob "bytes" optional=#true {{ validate {{ max_len 4096 }} }}
  field created "timestamp" {{ validate {{ lt_now }} }}
  field ttl "duration" default="60s" {{ validate {{ gte "1s"; lte "86400s" }} }}
  field kind "E{i}" default="alpha" {{ validate {{ not_in "delta" }} }}
  field id "string" {{ validate {{ uuid }} }}
"#
    );
    if !i.is_multiple_of(DEPTH) {
        let _ = writeln!(s, "  field next \"U{}\" optional=#true", i - 1);
    }
    let _ = write!(
        s,
        r#"}}

struct T{i} {{
  field count "u64" {{ validate {{ lte 1000000; multiple_of 2 }} }}
  field codes "list<i32>" optional=#true {{ validate {{ min_items 1; items {{ gt -100; lt 100 }} }} }}
  field limit "L{i}" default=10
}}

union U{i} tag="type" {{
  doc "Tagged union {i}."
  validate {{ required }}
  variant s "S{i}"
  variant t "T{i}"
}}

union L{i} {{
  variant count "u64"
  variant preset "string"
}}
"#
    );
}

fn keys(s: &mut String, g: usize, modules: usize) {
    let m = (g * 7) % modules.max(1);
    let _ = write!(
        s,
        r#"
keys "group{g}" {{
  doc "Keys group {g}."
  key "flag.enabled" "bool" default=#false {{ label "Enabled"; widget "toggle" }}
  key "timeout" "duration" default="600s" {{ validate {{ gte "1s"; lte "86400s" }} }}
  key "temperature" "f64" default=0.7 {{ validate {{ gte 0.0; lte 2.0 }} }}
  key "max_turns" "u32" default=20 {{ validate {{ gte 1; lte 1000 }}; unit "turns" }}
  key "model" "string" {{ validate {{ pattern "^[a-z0-9.-]+$"; max_len 100 }} }}
  key "api_key" "string" {{ secret; validate {{ min_len 20 }} }}
  key "kind" "E{m}" default="beta"
  key "limit" "L{m}" default=50
  key "servers" "map<string,U{m}>" default="{{}}"
  key "hosts" "list<string>" default="[]" {{ validate {{ items {{ hostname }} }} }}
}}
"#
    );
}

fn service(s: &mut String, v: usize, modules: usize) {
    let _ = writeln!(s, "\nservice Service{v} {{");
    for r in 0..5 {
        let a = (v * 5 + r) % modules.max(1);
        let _ = writeln!(
            s,
            "  rpc Call{r} input=\"S{a}\" output=\"T{a}\" idempotency=\"no_side_effects\""
        );
    }
    s.push_str("}\n");
}
