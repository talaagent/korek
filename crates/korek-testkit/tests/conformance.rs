//! Every vector against the loader (as examples) and the generated Rust.

#![allow(clippy::unwrap_used)]

#[allow(
    clippy::pedantic,
    clippy::std_instead_of_alloc,
    clippy::std_instead_of_core,
    dead_code
)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/vectors.rs"));
}

use korek_runtime::Validate as _;

/// Deserializes `{"v": value}` into the case's struct and validates it.
macro_rules! dispatch {
    ($name:expr, $json:expr, [$($case:literal => $ty:ident),* $(,)?]) => {
        match $name {
            $($case => serde_json::from_value::<generated::$ty>($json).map_or(false, |x| x.validate().is_ok()),)*
            other => panic!("no struct for case {other}"),
        }
    };
}

#[test]
fn generated_rust_agrees_with_the_vectors() {
    let mismatches = korek_testkit::run(|c, v| {
        let json = serde_json::json!({ "v": v });
        dispatch!(c.name.as_str(), json, [
            "min_len" => VMinLen, "max_len" => VMaxLen, "len" => VLen, "max_bytes" => VMaxBytes,
            "pattern" => VPattern, "prefix" => VPrefix, "suffix" => VSuffix, "contains" => VContains,
            "not_contains" => VNotContains, "in" => VIn, "not_in" => VNotIn, "const" => VConst,
            "email" => VEmail, "uuid" => VUuid, "ulid" => VUlid, "ipv4" => VIpv4, "ipv6" => VIpv6,
            "url" => VUrl, "date" => VDate, "datetime" => VDatetime, "e164" => VE164,
            "lowercase" => VLowercase, "base64" => VBase64, "int_range" => VIntRange,
            "exclusive" => VExclusive, "multiple_of" => VMultipleOf, "float_range" => VFloatRange,
            "int_in" => VIntIn, "bool_const" => VBoolConst, "min_items" => VMinItems,
            "unique" => VUnique, "items" => VItems, "pairs" => VPairs, "duration" => VDuration,
            "timestamp" => VTimestamp,
        ])
    });
    assert!(mismatches.is_empty(), "{mismatches:#?}");
}

#[test]
fn the_loader_agrees_with_the_vectors() {
    let mismatches = korek_testkit::run(|c, v| {
        let lit = match v {
            serde_json::Value::String(s) => format!("{s:?}"),
            serde_json::Value::Bool(b) => format!("#{b}"),
            serde_json::Value::Number(n) => n.to_string(),
            other => format!("{:?}", other.to_string()),
        };
        let src = format!(
            "struct S {{ field v {:?} {{ validate {{ {} }}; example {lit} }} }}",
            c.ty, c.validate
        );
        korek_core::Loader::new()
            .load_sources(
                vec![korek_core::SourceFile {
                    path: "v.kdl".into(),
                    contents: src,
                }],
                None,
            )
            .is_ok()
    });
    assert!(mismatches.is_empty(), "{mismatches:#?}");
}

/// `schema/vectors.kdl` (what the Zod test's generated code comes from) is
/// up to date with `vectors/rules.json`.
#[test]
fn the_vectors_schema_is_fresh() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/schema/vectors.kdl");
    let want = korek_testkit::schema();
    if std::env::var_os("KOREK_BLESS").is_some() {
        std::fs::write(path, &want).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(path).unwrap_or_default(),
        want,
        "rerun with KOREK_BLESS=1"
    );
}

/// The benchmark schema loads cleanly.
#[test]
fn the_synthetic_schema_loads() {
    let src = korek_testkit::synthetic::schema(korek_testkit::synthetic::Size::LARGE);
    let loaded = korek_core::Loader::new()
        .load_sources(
            vec![korek_core::SourceFile {
                path: "bench.kdl".into(),
                contents: src,
            }],
            None,
        )
        .unwrap_or_else(|f| panic!("{f}"));
    assert_eq!(loaded.schema.types.len(), 2000);
    assert_eq!(
        loaded
            .schema
            .keys
            .iter()
            .map(|g| g.keys.len())
            .sum::<usize>(),
        500
    );
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
}
