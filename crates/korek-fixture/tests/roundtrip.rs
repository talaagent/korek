//! The generated types read and write the JSON the other targets use.

use korek_fixture::example::{Backend, Constraint, Limit, McpServer, McpStdio, claude, session};
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
            env: None,
        })
    );
    assert_eq!(serde_json::to_value(&s).unwrap(), v);
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

#[test]
fn keys_carry_defaults_and_metadata() {
    assert_eq!(session::BACKEND.default_value(), Some(Backend::Claude));
    assert_eq!(session::TIMEOUT_S.default_value(), Some(600));
    assert_eq!(
        session::TIMEOUT_S.constraints,
        &[Constraint::Min(1.0), Constraint::Max(86400.0)]
    );
    assert_eq!(session::STEER_ENABLED.path(), "session.steer.enabled");
    assert_eq!(session::TURN_LIMIT.default_value(), Some(Limit::Count(50)));
    assert!(claude::MODEL.default.is_none());
    // The typed default and its JSON form agree.
    let json: serde_json::Value =
        serde_json::from_str(claude::MCP_SERVERS.default_json.unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(claude::MCP_SERVERS.default_value().unwrap()).unwrap(),
        json
    );
    assert_eq!(session::NAMES.len(), 5);
}
