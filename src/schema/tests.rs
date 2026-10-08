use super::*;
use serde_json::{Value, json};

fn document() -> Value {
    json!({
        "schema_version": 1,
        "servers": {
            "local": {
                "transport": {
                    "type": "stdio", "command": "npx",
                    "args": ["-y", "example-mcp@1.2.3", {"env": "ARG_TOKEN"}],
                    "env": {"TOKEN": {"env": "LOCAL_TOKEN"}, "REGION": "us-east-1"},
                    "env_vars": ["HOME"], "cwd": "./tools"
                },
                "settings": {
                    "enabled": false, "required": true,
                    "startup_timeout_sec": 15.5, "tool_timeout_sec": 45.0,
                    "enabled_tools": ["read", "write"], "disabled_tools": ["write"]
                }
            },
            "remote": {
                "transport": {
                    "type": "http", "url": "https://mcp.example.com/mcp",
                    "bearer_token": {"env": "REMOTE_TOKEN"},
                    "headers": {"X-Region": "east", "Authorization": {"env": "AUTH_HEADER"}}
                }
            }
        }
    })
}

fn parse(value: &Value) -> Result<StackV1, ValidationError> {
    StackV1::from_yaml(&value.to_string())
}

#[test]
fn supported_fields_round_trip_without_resolving_references() {
    let stack = parse(&document()).unwrap();
    let first = serde_json::to_string_pretty(&stack).unwrap();
    let restored = StackV1::from_yaml(&first).unwrap();
    let second = serde_json::to_string_pretty(&restored).unwrap();
    assert_eq!(first, second);
    let normalized: Value = serde_json::from_str(&first).unwrap();
    let expected = document();
    assert_eq!(normalized["servers"]["local"], expected["servers"]["local"]);
    assert_eq!(
        normalized["servers"]["remote"]["transport"],
        expected["servers"]["remote"]["transport"]
    );
    assert_eq!(normalized["servers"]["remote"]["settings"]["enabled"], true);
    assert!(first.find("local").unwrap() < first.find("remote").unwrap());
    assert_eq!(
        normalized["servers"]["local"]["transport"]["env"]["TOKEN"],
        json!({"env":"LOCAL_TOKEN"})
    );
}

#[test]
fn stacks_are_client_independent_and_reject_legacy_compatibility_fields() {
    let minimal = json!({"schema_version":1,"servers":{}});
    let stack = parse(&minimal).unwrap();
    assert_eq!(serde_json::to_value(&stack).unwrap(), minimal);
    let mut legacy = document();
    legacy["compatibility"] = json!({"codex":">=0.100.0"});
    assert_eq!(
        parse(&legacy).err().unwrap(),
        ValidationError::InvalidDocument
    );
}

#[test]
fn rejects_unsupported_versions_before_decoding_future_fields() {
    for version in [0, 2, u64::MAX] {
        let value = json!({"schema_version": version, "future_format": true});
        assert_eq!(
            parse(&value).err().unwrap(),
            ValidationError::UnsupportedVersion
        );
    }
}

#[test]
fn rejects_unknown_fields_at_every_level_and_invalid_transports() {
    for pointer in [
        "",
        "/servers/local",
        "/servers/local/transport",
        "/servers/local/settings",
        "/servers/local/transport/env/TOKEN",
    ] {
        let mut value = document();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unsupported".into(), json!("fixture-secret"));
        assert_eq!(
            parse(&value).err().unwrap(),
            ValidationError::InvalidDocument,
            "{pointer}"
        );
    }
    let mut value = document();
    value["servers"]["local"]["transport"]["type"] = json!("sdk");
    assert_eq!(
        parse(&value).err().unwrap(),
        ValidationError::InvalidDocument
    );
    value = document();
    value["servers"]["local"]["transport"]["url"] = json!("https://example.com");
    assert_eq!(
        parse(&value).err().unwrap(),
        ValidationError::InvalidDocument
    );
}

#[test]
fn rejects_duplicate_keys_instead_of_silently_replacing_definitions() {
    let version = r#""schema_version":1"#;
    let server = r#"{"transport":{"type":"stdio","command":"example"}}"#;
    let duplicates = [
        format!(r#"{{{version},"servers":{{"same":{server},"same":{server}}}}}"#),
        format!(
            r#"{{{version},"servers":{{"local":{{"transport":{{"type":"stdio","command":"example","env":{{"TOKEN":"first","TOKEN":"second"}}}}}}}}}}"#
        ),
        format!(
            r#"{{{version},"servers":{{"remote":{{"transport":{{"type":"http","url":"https://example.com","headers":{{"X-Token":"first","X-Token":"second"}}}}}}}}}}"#
        ),
    ];
    for document in duplicates {
        assert_eq!(
            StackV1::from_yaml(&document).err().unwrap(),
            ValidationError::InvalidDocument
        );
    }
}

#[test]
fn validates_server_names_and_required_fields() {
    for name in ["", " leading", "trailing ", "control\n"] {
        let mut value = document();
        let server = value["servers"]
            .as_object_mut()
            .unwrap()
            .remove("local")
            .unwrap();
        value["servers"]
            .as_object_mut()
            .unwrap()
            .insert(name.into(), server);
        assert_eq!(
            parse(&value).err().unwrap(),
            ValidationError::InvalidServerName
        );
    }
    let mut value = document();
    value["servers"]["local"]["transport"]
        .as_object_mut()
        .unwrap()
        .remove("command");
    assert_eq!(
        parse(&value).err().unwrap(),
        ValidationError::InvalidDocument
    );
    value["servers"]["local"]["transport"]["command"] = json!("  ");
    assert_eq!(
        parse(&value).err().unwrap(),
        ValidationError::InvalidProcess
    );
    assert!(parse(&json!({"schema_version":1,"servers":{}})).is_ok());
}

#[test]
fn validates_secret_references_and_forwarded_environment_names() {
    for name in ["", "9TOKEN", "WITH SPACE", "TOKEN-NAME", "${TOKEN}"] {
        let mut value = document();
        value["servers"]["local"]["transport"]["env"]["TOKEN"] = json!({"env":name});
        assert_eq!(
            parse(&value).err().unwrap(),
            ValidationError::InvalidSecretReference
        );
        value = document();
        value["servers"]["remote"]["transport"]["bearer_token"] = json!({"env":name});
        assert_eq!(
            parse(&value).err().unwrap(),
            ValidationError::InvalidSecretReference
        );
    }
    let mut value = document();
    value["servers"]["local"]["transport"]["env_vars"] = json!(["TOKEN", "TOKEN"]);
    assert_eq!(
        parse(&value).err().unwrap(),
        ValidationError::InvalidEnvironment
    );
    value["servers"]["local"]["transport"]["env_vars"] = json!(["BAD-NAME"]);
    assert_eq!(
        parse(&value).err().unwrap(),
        ValidationError::InvalidEnvironment
    );
    value = document();
    value["servers"]["remote"]["transport"]["bearer_token"] = json!("fixture-secret");
    assert_eq!(
        parse(&value).err().unwrap(),
        ValidationError::InvalidDocument
    );
}

#[test]
fn validates_http_endpoints_and_headers() {
    for url in [
        "relative/path",
        "ftp://example.com",
        "https://user:fixture-secret@example.com",
        "https://example.com/#fragment",
        "https://",
        " https://example.com",
        "https://example.com/\n",
    ] {
        let mut value = document();
        value["servers"]["remote"]["transport"]["url"] = json!(url);
        assert_eq!(
            parse(&value).err().unwrap(),
            ValidationError::InvalidEndpoint,
            "{url}"
        );
    }
    for headers in [
        json!({"Bad Header":"value"}),
        json!({"X-Token":"value\r\nInjected: true"}),
        json!({"X-Token":"first", "x-token":"second"}),
    ] {
        let mut value = document();
        value["servers"]["remote"]["transport"]["headers"] = headers;
        assert_eq!(parse(&value).err().unwrap(), ValidationError::InvalidHeader);
    }
}

#[test]
fn validates_timeouts_and_tool_lists() {
    for timeout in [0.0, -1.0] {
        let mut value = document();
        value["servers"]["local"]["settings"]["tool_timeout_sec"] = json!(timeout);
        assert_eq!(
            parse(&value).err().unwrap(),
            ValidationError::InvalidTimeout
        );
    }
    for tools in [json!([""]), json!(["read", "read"]), json!([" read"])] {
        let mut value = document();
        value["servers"]["local"]["settings"]["enabled_tools"] = tools;
        assert_eq!(parse(&value).err().unwrap(), ValidationError::InvalidTools);
    }
}

#[test]
fn malformed_documents_never_echo_input_values() {
    for document in [
        "",
        "not-json",
        r#"{"schema_version": "fixture-secret"}"#,
        r#"{"schema_version":1,"servers":"fixture-secret"}"#,
    ] {
        let error = StackV1::from_yaml(document).err().unwrap();
        assert!(!error.to_string().contains("fixture-secret"));
        assert!(!format!("{error:?}").contains("fixture-secret"));
    }
}

#[test]
fn duplicate_struct_fields_and_malformed_references_are_rejected() {
    for input in [
        r#"{"schema_version":1,"schema_version":1,"servers":{}}"#,
        r#"{"schema_version":1,"servers":{"local":{"transport":{"type":"stdio","command":"one","command":"two"}}}}"#,
        r#"{"schema_version":1,"servers":{},"servers":{}}"#,
    ] {
        assert_eq!(
            StackV1::from_yaml(input).err().unwrap(),
            ValidationError::InvalidDocument
        );
    }
    for reference in [
        json!({}),
        json!({"env":null}),
        json!({"env":42}),
        json!({"env":"TOKEN","provider":"other"}),
    ] {
        let mut value = document();
        value["servers"]["local"]["transport"]["args"] = json!([reference]);
        assert_eq!(
            parse(&value).err().unwrap(),
            ValidationError::InvalidDocument
        );
    }
}

#[test]
fn rejects_nul_in_literal_process_values() {
    let mut value = document();
    value["servers"]["local"]["transport"]["args"] = json!(["bad\0value"]);
    assert_eq!(parse(&value).err().unwrap(), ValidationError::InvalidValue);
    value = document();
    value["servers"]["local"]["transport"]["env"]["TOKEN"] = json!("bad\0value");
    assert_eq!(parse(&value).err().unwrap(), ValidationError::InvalidValue);
}

#[test]
fn remote_transports_preserve_protocol_and_enforce_url_schemes() {
    for (kind, valid, invalid) in [
        ("sse", "https://example.com/sse", "wss://example.com/sse"),
        (
            "websocket",
            "wss://example.com/mcp",
            "https://example.com/mcp",
        ),
    ] {
        let mut value = document();
        value["servers"]["remote"]["transport"]["type"] = json!(kind);
        value["servers"]["remote"]["transport"]["url"] = json!(valid);
        let stack = parse(&value).unwrap();
        assert_eq!(
            serde_json::to_value(stack).unwrap()["servers"]["remote"]["transport"],
            value["servers"]["remote"]["transport"]
        );
        value["servers"]["remote"]["transport"]["url"] = json!(invalid);
        assert_eq!(
            parse(&value).err().unwrap(),
            ValidationError::InvalidEndpoint
        );
    }
}

#[test]
fn configuration_fields_and_secret_references_round_trip_without_client_ownership() {
    let configs = [
        (
            "codex",
            json!({
                "command":"example", "env_vars":[{"name":"TOKEN","source":"remote"}],
                "environment_id":"remote", "auth":"ema_auth", "startup_timeout_ms":0,
                "oauth":{"client_id":"id", "client_secret":{"$env":"CLIENT_SECRET"}},
                "tools":{"read":{"approval_mode":"prompt","output_token_limit":512}},
                "future_field":[null, true, 42, {"nested":{"$env":"TOKEN"}}]
            }),
        ),
        (
            "claude_code",
            json!({
                "type":"streamable-http", "url":"https://${HOST:-example.com}/mcp",
                "oauth":{"clientId":"id", "callbackPort":8080,"scopes":"read write"},
                "headersHelper":"./auth.sh", "alwaysLoad":true, "timeout":600000,
                "bareElicitationCapability":true
            }),
        ),
        (
            "claude_desktop",
            json!({"command":"example", "args":["${HOME}/data"],
            "env":{"TOKEN":{"$env":"TOKEN"}}}),
        ),
    ];
    for (_, config) in configs {
        let value = json!({"schema_version":1,"servers":{"Friendly server é":{
            "config":config
        }}});
        let encoded = serde_json::to_value(parse(&value).unwrap()).unwrap();
        assert_eq!(encoded, value);
    }
}

#[test]
fn configuration_definitions_reject_client_labels_and_mixed_forms() {
    for server in [
        json!({"client":"codex", "config":{"command":"tool"}}),
        json!({"client":"claude_code", "config":{"command":"tool"}}),
        json!({"client":"claude_desktop", "config":{"command":"tool"}}),
        json!({"config":{}, "settings":{}}),
        json!({"config":{}, "transport":{"type":"stdio","command":"example"}}),
        json!({"config":[], "extra":true}),
    ] {
        assert_eq!(
            parse(&json!({"schema_version":1,"servers":{"local":server}}))
                .err()
                .unwrap(),
            ValidationError::InvalidDocument
        );
    }
}

#[test]
fn native_nested_duplicates_and_malformed_secret_markers_are_rejected() {
    for config in [
        r#"{"oauth":{"client_id":"first","client_id":"second"}}"#,
        r#"{"args":[{"nested":{"same":1,"same":2}}]}"#,
        r#"{"env":{"TOKEN":"first"},"env":{"TOKEN":"second"}}"#,
    ] {
        let input =
            format!(r#"{{"schema_version":1,"servers":{{"native":{{"config":{config}}}}}}}"#);
        assert_eq!(
            StackV1::from_yaml(&input).err().unwrap(),
            ValidationError::InvalidDocument
        );
    }
    for marker in [
        json!({"$env":"bad-name"}),
        json!({"$env":null}),
        json!({"$env":"TOKEN","fallback":"fixture-secret"}),
    ] {
        let value = json!({"schema_version":1,"servers":{"native":{
            "config":{"oauth":{"client_secret":marker}}
        }}});
        let error = parse(&value).err().unwrap();
        assert_eq!(error, ValidationError::InvalidSecretReference);
        assert!(!error.to_string().contains("fixture-secret"));
    }
}

#[test]
fn yaml_round_trip_preserves_string_types_and_nested_values() {
    let mut value = document();
    value["servers"]["local"]["transport"]["args"] =
        json!(["true", "null", "42", "line\nbreak", "a: b", "# comment"]);
    let stack = parse(&value).unwrap();
    let yaml = yaml_serde::to_string(&stack).unwrap();
    let restored = StackV1::from_yaml(&yaml).unwrap();
    assert_eq!(
        serde_json::to_value(&restored).unwrap(),
        serde_json::to_value(&stack).unwrap()
    );
}

#[test]
fn yaml_rejects_duplicates_and_unsupported_versions() {
    for input in [
        "schema_version: 1\nservers: {}\nservers: {}",
        "schema_version: 1\nservers:\n  local:\n    config:\n      env: {TOKEN: one, TOKEN: two}",
    ] {
        assert_eq!(
            StackV1::from_yaml(input).err().unwrap(),
            ValidationError::InvalidDocument
        );
    }
    assert_eq!(
        StackV1::from_yaml("schema_version: 2\nfuture: true")
            .err()
            .unwrap(),
        ValidationError::UnsupportedVersion
    );
}

#[test]
fn exporter_dispatch_rejects_unknown_versions_and_round_trips_v1() {
    let mut stack = Stack::V1(parse(&document()).unwrap());
    let yaml = crate::exporters::to_yaml(&stack).unwrap();
    let restored = crate::importers::from_yaml(&yaml).unwrap();
    assert!(matches!(restored, Stack::V1(_)));
    assert_eq!(
        serde_json::to_value(&restored).unwrap(),
        serde_json::to_value(&stack).unwrap()
    );
    let Stack::V1(inner) = &mut stack;
    inner.schema_version = 2;
    assert_eq!(
        crate::exporters::to_yaml(&stack).err().unwrap(),
        ValidationError::UnsupportedVersion
    );
}
