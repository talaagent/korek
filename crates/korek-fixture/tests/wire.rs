//! `bytes` at any depth is base64 in JSON, and `rust_duration` fields are
//! `Duration` in Rust and the integer in JSON.

#![allow(clippy::unwrap_used)]

extern crate alloc;

use alloc::collections::BTreeMap;
use core::time::Duration;

use korek_fixture::wire::{Blob, Blobs, Holder, Timing, keys};
use korek_runtime::Validate as _;
use serde_json::json;

#[test]
fn nested_bytes_are_base64() {
    let v = json!({
        "one": "AQI=",
        "maybe": "",
        "list": ["AA==", "/w=="],
        "map": {"a": "AAE="},
        "nested": {"x": ["AQ==", "AgM="], "y": []},
        "deep": [["AA=="], []],
    });
    let b: Blobs = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(b.one, [1, 2]);
    assert_eq!(b.maybe, Some(vec![]));
    assert_eq!(b.list, [vec![0], vec![255]]);
    assert_eq!(b.map, BTreeMap::from([("a".into(), vec![0, 1])]));
    assert_eq!(b.nested.as_ref().unwrap()["x"], [vec![1], vec![2, 3]]);
    assert_eq!(b.deep, [vec![vec![0]], vec![]]);
    assert_eq!(serde_json::to_value(&b).unwrap(), v);
    assert!(b.validate().is_ok());

    // Absent: optional is skipped, the default applies.
    let b: Blobs = serde_json::from_value(json!({"one": "", "list": [], "map": {}})).unwrap();
    assert_eq!((&b.maybe, &b.nested), (&None, &None));
    assert_eq!(b.deep, Blobs::default_deep());
    assert_eq!(b.deep, [vec![vec![0, 1]]]);

    // Rules still see the bytes; bad base64 is an error.
    let mut long = b.clone();
    long.list = vec![vec![0; 5]];
    assert_eq!(long.validate().unwrap_err().0[0].path, "list[0]");
    assert!(serde_json::from_value::<Blobs>(json!({"one": "", "list": ["!"], "map": {}})).is_err());
}

#[test]
fn untagged_union_bytes_variants() {
    for (v, want) in [
        (json!("AQ=="), Blob::Raw(vec![1])),
        (json!(["AQ==", "Ag=="]), Blob::Many(vec![vec![1], vec![2]])),
    ] {
        let b: Blob = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(b, want);
        assert_eq!(serde_json::to_value(&b).unwrap(), v);
    }
}

#[test]
fn durations_keep_the_integer_form() {
    let v = json!({"timeout_ms": 250, "ttl": 90, "retry": 3, "count": 7});
    let t: Timing = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(t.timeout_ms, Duration::from_millis(250));
    assert_eq!(t.ttl, Some(Duration::from_secs(90)));
    assert_eq!(t.retry, Duration::from_secs(180));
    assert_eq!(t.count, 7);
    assert_eq!(serde_json::to_value(&t).unwrap(), v);

    // Defaults and absent optionals.
    let t: Timing = serde_json::from_value(json!({"retry": 0, "count": 0})).unwrap();
    assert_eq!((t.timeout_ms, t.ttl), (Duration::from_secs(30), None));
    assert_eq!(
        serde_json::to_value(&t).unwrap(),
        json!({"timeout_ms": 30000, "retry": 0, "count": 0})
    );
    assert_eq!(Holder::default().timing.retry, Duration::from_secs(120));

    // Rules apply to the integer count of the unit.
    let mut bad = t.clone();
    bad.timeout_ms = Duration::ZERO;
    bad.retry = Duration::from_secs(11 * 60 + 1);
    let errs = bad.validate().unwrap_err();
    let paths: Vec<_> = errs.0.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, ["timeout_ms", "retry"]);
    // Negative counts do not fit a Duration.
    assert!(serde_json::from_value::<Timing>(json!({"retry": -1, "count": 0})).is_err());

    // Keys too.
    let k = keys::WIRE_TIMEOUT;
    assert_eq!(k.default_value(), Some(Duration::from_millis(1500)));
    assert_eq!(k.unit, Some("milliseconds"));
    assert!(k.validate(&Duration::ZERO).is_err());
    assert!(k.validate(&Duration::from_millis(1)).is_ok());
}
