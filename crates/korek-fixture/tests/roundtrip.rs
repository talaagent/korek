//! The generated types read and write the canonical JSON, validate, and
//! carry key metadata.

#![allow(clippy::unwrap_used, clippy::assertions_on_constants)]

use korek_fixture::example::{
    Backend, Contact, Limit, McpHttp, McpServer, McpStdio, SearchRequest, keys,
};
use korek_runtime::Validate as _;
use serde_json::json;

#[test]
fn tagged_union_round_trips() {
    let v = json!({"type": "stdio", "command": "mcp", "args": ["-v"]});
    let s: McpServer = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(
        s,
        McpServer::Stdio(McpStdio {
            command: "mcp".into(),
            args: Some(vec!["-v".into()]),
            env: None
        })
    );
    assert_eq!(serde_json::to_value(&s).unwrap(), v);
}

#[test]
fn defaults_fill_absent_fields() {
    let h: McpHttp = serde_json::from_value(json!({"url": "https://x.test"})).unwrap();
    assert_eq!(h.timeout_ms, 30000);
    assert_eq!(
        SearchRequest {
            query: "a".into(),
            limit: SearchRequest::default_limit()
        }
        .limit,
        20
    );
}

#[test]
fn untagged_union_and_enum() {
    assert_eq!(
        serde_json::from_value::<Limit>(json!(5)).unwrap(),
        Limit::Count(5)
    );
    assert_eq!(
        serde_json::from_value::<Limit>(json!("max")).unwrap(),
        Limit::Preset("max".into())
    );
    assert_eq!(
        serde_json::to_value(Backend::Codex).unwrap(),
        json!("codex")
    );
}

fn contact() -> Contact {
    serde_json::from_value(json!({
        "id": "123e4567-e89b-12d3-a456-426614174000",
        "name": "Ada",
        "emails": ["ada@example.com"],
        "avatar": "AAEC",
        "created": "2020-01-01T00:00:00Z",
    }))
    .unwrap()
}

#[test]
fn bytes_are_base64() {
    let c = contact();
    assert_eq!(c.avatar.as_deref(), Some(&[0u8, 1, 2][..]));
    assert_eq!(serde_json::to_value(&c).unwrap()["avatar"], "AAEC");
}

#[test]
fn validate_reports_every_failure_with_paths() {
    assert_eq!(contact().validate(), Ok(()));
    let mut c = contact();
    c.id = "nope".into();
    c.emails = vec!["a@example.com".into(), "a@example.com".into(), "bad".into()];
    c.score = Some(2.0);
    let errs = c.validate().unwrap_err();
    let got: Vec<(String, &str)> = errs.0.iter().map(|e| (e.path.clone(), e.rule)).collect();
    assert_eq!(
        got,
        [
            ("id".to_owned(), "uuid"),
            ("emails".to_owned(), "unique"),
            ("emails[2]".to_owned(), "email"),
            ("score".to_owned(), "lte"),
        ]
    );
    let bad = McpServer::Http(McpHttp {
        url: "http://x.test".into(),
        timeout_ms: 0,
    });
    let errs = bad.validate().unwrap_err();
    assert_eq!(errs.0[0].message, "MCP servers must use HTTPS");
    assert_eq!(errs.0[1].path, "timeout_ms");
}

#[test]
#[allow(deprecated)]
fn keys_carry_defaults_and_metadata() {
    assert_eq!(keys::SESSION_BACKEND.default_value(), Some(Backend::Claude));
    assert_eq!(
        keys::SESSION_TIMEOUT.default_value().as_deref(),
        Some("600s")
    );
    assert!(keys::SESSION_TIMEOUT.validate(&"0.5s".to_owned()).is_err());
    assert!(keys::SESSION_TIMEOUT.validate(&"60s".to_owned()).is_ok());
    assert_eq!(keys::SESSION_STEER_ENABLED.path, "session.steer.enabled");
    assert_eq!(
        keys::SESSION_TURN_LIMIT.default_value(),
        Some(Limit::Count(50))
    );
    assert!(keys::CLAUDE_API_KEY.secret);
    assert_eq!(keys::SESSION_MAX_TURNS.deprecated, Some("use turn_limit"));
    let json: serde_json::Value =
        serde_json::from_str(keys::CLAUDE_MCP_SERVERS.default_json.unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(keys::CLAUDE_MCP_SERVERS.default_value().unwrap()).unwrap(),
        json
    );
    assert_eq!(keys::ALL.len(), 9);
}
