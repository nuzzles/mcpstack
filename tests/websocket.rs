use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

fn run(
    harness: &str,
    operation: &str,
    stack: Option<&Path>,
    config: &Path,
    flags: &[&str],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcpstack"));
    command.args([
        "--non-interactive",
        "--no-color",
        "--skip-version-check",
        harness,
        operation,
    ]);
    if let Some(stack) = stack {
        command.arg(stack);
    }
    command
        .arg("--config")
        .arg(config)
        .args(flags)
        .env_remove("RUST_LOG")
        .env_remove("MCPSTACK_WS_RUNTIME_TOKEN")
        .env("MCPSTACK_WS_LITERAL_TOKEN", "new-fixture-secret")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn success(output: Output) -> String {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn websocket_export_round_trips_both_schemes_headers_and_opt_in_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let settings = dir.path().join("settings.json");
    let stack = dir.path().join("stack.yaml");
    let original = r#"{
  "theme" : "dark",
  "mcpServers": {
    "events": {"type":"ws","url":"wss://example.com/events","headers":{"Authorization":"Bearer ${MCPSTACK_WS_RUNTIME_TOKEN}","X-API-Key":"fixture-secret"}},
    "local": {"type":"ws","url":"ws://127.0.0.1:9/events","headers":{}}
  }
}"#;
    let policies = r#"{"permissions":{"allow":["Read(*)","mcp__events__read"],"deny":["mcp__events__delete"]},"other":true}"#;
    fs::write(&config, original).unwrap();
    fs::write(&settings, policies).unwrap();
    let shown = success(run("claude", "show", None, &config, &[]));
    assert!(!shown.contains("fixture-secret"));
    let masked: Value = yaml_serde::from_str(&shown).unwrap();
    assert_eq!(
        masked["servers"]["events"]["transport"]["type"],
        "websocket"
    );
    assert_eq!(masked["servers"]["local"]["transport"]["type"], "websocket");
    assert_eq!(
        masked["servers"]["events"]["transport"]["headers"]["Authorization"],
        json!({"env":"MCPSTACK_WS_RUNTIME_TOKEN","prefix":"Bearer "})
    );
    assert!(
        masked["servers"]["events"]["transport"]["headers"]["X-API-Key"]["value_env"].is_string()
    );
    assert!(masked["servers"]["events"].get("permissions").is_none());
    success(run(
        "claude",
        "export",
        Some(&stack),
        &config,
        &["--expose-secrets", "--include-permissions"],
    ));
    let exported: Value = yaml_serde::from_str(&fs::read_to_string(&stack).unwrap()).unwrap();
    assert_eq!(
        exported["servers"]["events"]["permissions"]["allow"],
        json!(["read"])
    );
    assert_eq!(
        exported["servers"]["events"]["permissions"]["deny"],
        json!(["delete"])
    );
    for (operation, flags) in [
        ("diff", vec!["--include-permissions"]),
        ("import", vec!["-y", "--include-permissions"]),
        ("use", vec!["-y", "--include-permissions"]),
    ] {
        let result = success(run("claude", operation, Some(&stack), &config, &flags));
        assert!(result.contains("No changes"), "{operation}: {result}");
        assert_eq!(fs::read_to_string(&config).unwrap(), original);
        assert_eq!(fs::read_to_string(&settings).unwrap(), policies);
    }
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
}

#[test]
fn websocket_previews_imports_and_switches_preserve_settings_and_redact_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yaml");
    let original = r#"{"theme" : "dark", "mcpServers":{"old":{"command":"old-tool"},"events":{"type":"http","url":"https://example.com/events","headers":{"X-API-Key":"old-fixture-secret"}}}}"#;
    fs::write(&config, original).unwrap();
    fs::write(&stack, "schema_version: 1\nservers:\n  events:\n    transport:\n      type: websocket\n      url: wss://example.com/events\n      headers:\n        Authorization: {env: MCPSTACK_WS_RUNTIME_TOKEN, prefix: 'Bearer '}\n        X-API-Key: {value_env: MCPSTACK_WS_LITERAL_TOKEN}\n  local:\n    transport: {type: websocket, url: 'ws://127.0.0.1:9/events'}\n").unwrap();
    for (operation, flags) in [
        ("diff", vec![]),
        ("import", vec!["--dry-run", "-y"]),
        ("use", vec!["--dry-run"]),
    ] {
        let preview = success(run("claude", operation, Some(&stack), &config, &flags));
        assert!(preview.contains("Modified: \"events\""), "{preview}");
        assert!(preview.contains("Added: \"local\""), "{preview}");
        assert!(!preview.contains("fixture-secret"), "{preview}");
        if operation == "diff" {
            assert!(preview.contains("wss://example.com/events"));
            assert!(preview.contains("\"ws\""));
        }
        if operation == "use" {
            assert!(preview.contains("Deleted: \"old\""));
        }
        assert_eq!(fs::read_to_string(&config).unwrap(), original);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }
    success(run("claude", "import", Some(&stack), &config, &["-y"]));
    let imported = read_json(&config);
    assert_eq!(imported["mcpServers"]["events"]["type"], "ws");
    assert_eq!(
        imported["mcpServers"]["events"]["headers"],
        json!({
            "Authorization":"Bearer ${MCPSTACK_WS_RUNTIME_TOKEN}", "X-API-Key":"new-fixture-secret"
        })
    );
    assert!(imported["mcpServers"].get("old").is_some());
    assert!(
        fs::read_to_string(&config)
            .unwrap()
            .contains("\"theme\" : \"dark\"")
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("claude.json.~1~")).unwrap(),
        original
    );
    let before = fs::read(&config).unwrap();
    assert!(
        success(run("claude", "import", Some(&stack), &config, &["-y"])).contains("No changes")
    );
    assert_eq!(fs::read(&config).unwrap(), before);
    success(run("claude", "use", Some(&stack), &config, &["-y"]));
    assert!(read_json(&config)["mcpServers"].get("old").is_none());
    assert_eq!(
        fs::read(dir.path().join("claude.json.~2~")).unwrap(),
        before
    );
    let before = fs::read(&config).unwrap();
    assert!(success(run("claude", "use", Some(&stack), &config, &["-y"])).contains("No changes"));
    assert_eq!(fs::read(&config).unwrap(), before);
    assert!(!dir.path().join("claude.json.~3~").exists());
    assert!(!success(run("claude", "show", None, &config, &[])).contains("fixture-secret"));
    let native_servers: serde_json::Map<String, Value> = read_json(&config)["mcpServers"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, config)| (name.clone(), json!({"config":config})))
        .collect();
    fs::write(
        &stack,
        json!({"schema_version":1,"servers":native_servers}).to_string(),
    )
    .unwrap();
    assert!(
        success(run("claude", "import", Some(&stack), &config, &["-y"])).contains("No changes")
    );
    assert_eq!(fs::read(&config).unwrap(), before);
    assert!(!dir.path().join("claude.json.~3~").exists());
}

#[test]
fn invalid_websocket_definitions_fail_without_partial_writes_or_exports() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yaml");
    let exported = dir.path().join("export.yaml");
    let original = r#"{"theme":"dark","mcpServers":{"keep":{"command":"tool"}}}"#;
    for invalid in [
        json!({"type":"ws","url":"https://example.com/private-fixture"}),
        json!({"type":"ws","url":"wss://user:private-fixture@example.com/events"}),
        json!({"type":"ws","url":"wss://example.com/events#private-fixture"}),
        json!({"type":"ws","url":"wss://example.com/events\nprivate-fixture"}),
        json!({"type":"ws","url":"wss://example.com/events","command":"private-fixture"}),
        json!({"type":"ws","url":"wss://example.com/events","headers":{"Bad Header":"private-fixture"}}),
        json!({"type":"ws","url":"wss://example.com/events","headers":{"Authorization":"private-fixture\r\ninjected"}}),
        json!({"type":"ws","url":"wss://example.com/events","headers":{"Authorization":"private-fixture","authorization":"duplicate"}}),
        json!({"type":"ws","url":"wss://example.com/events","headersHelper":"private-fixture"}),
    ] {
        fs::write(&config, original).unwrap();
        fs::write(
            &stack,
            json!({"schema_version":1,"servers":{
                "valid":{"transport":{"type":"websocket","url":"ws://localhost:9/events"}},
                "invalid":{"config":invalid}
            }})
            .to_string(),
        )
        .unwrap();
        for operation in ["diff", "import", "use"] {
            let output = run(
                "claude",
                operation,
                Some(&stack),
                &config,
                if operation == "diff" { &[] } else { &["-y"] },
            );
            assert!(!output.status.success(), "{invalid}: {output:?}");
            assert!(!String::from_utf8_lossy(&output.stderr).contains("private-fixture"));
            assert_eq!(fs::read_to_string(&config).unwrap(), original);
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        }
        let native = json!({"mcpServers":{"invalid":invalid}}).to_string();
        fs::write(&config, &native).unwrap();
        for operation in ["show", "export"] {
            let output = run(
                "claude",
                operation,
                if operation == "export" {
                    Some(&exported)
                } else {
                    None
                },
                &config,
                &[],
            );
            assert!(!output.status.success(), "{invalid}: {output:?}");
            assert!(output.stdout.is_empty());
            assert!(!String::from_utf8_lossy(&output.stderr).contains("private-fixture"));
            assert_eq!(fs::read_to_string(&config).unwrap(), native);
            assert!(!exported.exists());
        }
    }
}

#[test]
fn websocket_listing_redacts_endpoints_and_keeps_transport_schemes_distinct() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let original = json!({"mcpServers": {
        "secure":{"type":"ws","url":"wss://user:fixture-secret@example.com/events?token=fixture-secret#fixture-secret","headers":{"Authorization":"fixture-secret"}},
        "local":{"type":"ws","url":"ws://localhost:9/events?token=fixture-secret"}
    }}).to_string();
    fs::write(&config, &original).unwrap();
    let listing = success(run("claude", "list", None, &config, &[]));
    assert_eq!(listing.matches("websocket").count(), 2);
    assert!(listing.contains("wss://example.com/events"));
    assert!(listing.contains("ws://localhost:9/events"));
    assert!(!listing.contains("fixture-secret"));
    assert!(!listing.contains("user"));
    assert_eq!(fs::read_to_string(&config).unwrap(), original);
    for (kind, url) in [
        ("ws", "https://example.com"),
        ("http", "wss://example.com"),
        ("sse", "ws://example.com"),
    ] {
        fs::write(
            &config,
            json!({"mcpServers":{"a":{"command":"good"},"z":{"type":kind,"url":url}}}).to_string(),
        )
        .unwrap();
        let output = run("claude", "list", None, &config, &[]);
        assert_eq!(output.status.code(), Some(5), "{output:?}");
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn codex_rejects_portable_and_native_websocket_imports_without_writes() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let stack = dir.path().join("stack.yaml");
    let original = "model='example'\n[mcp_servers.keep]\ncommand='tool'\n";
    fs::write(&config, original).unwrap();
    for server in [
        json!({"transport":{"type":"websocket","url":"wss://example.com/events"}}),
        json!({"transport":{"type":"websocket","url":"ws://localhost:9/events"}}),
        json!({"config":{"type":"ws","url":"wss://example.com/events"}}),
        json!({"config":{"url":"wss://example.com/events"}}),
    ] {
        fs::write(
            &stack,
            json!({"schema_version":1,"servers":{"events":server}}).to_string(),
        )
        .unwrap();
        for operation in ["diff", "import", "use"] {
            let output = run(
                "codex",
                operation,
                Some(&stack),
                &config,
                if operation == "diff" { &[] } else { &["-y"] },
            );
            assert!(!output.status.success(), "{output:?}");
            assert_eq!(fs::read_to_string(&config).unwrap(), original);
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        }
    }
}

#[test]
fn websocket_rejects_invalid_resolved_headers_without_exposing_or_writing_them() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yaml");
    let original = r#"{"mcpServers":{},"theme":"dark"}"#;
    fs::write(&config, original).unwrap();
    fs::write(&stack, "schema_version: 1\nservers:\n  events:\n    transport:\n      type: websocket\n      url: wss://example.com/events\n      headers:\n        Authorization: {value_env: MCPSTACK_WS_INVALID_HEADER}\n").unwrap();
    for operation in ["diff", "import", "use"] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mcpstack"));
        command
            .args([
                "--non-interactive",
                "--skip-version-check",
                "claude",
                operation,
            ])
            .arg(&stack)
            .arg("--config")
            .arg(&config)
            .env_remove("RUST_LOG")
            .env(
                "MCPSTACK_WS_INVALID_HEADER",
                "fixture-secret\r\ninjected-header",
            )
            .stdin(Stdio::null());
        if operation != "diff" {
            command.arg("-y");
        }
        let output = command.output().unwrap();
        assert!(!output.status.success(), "{output:?}");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("fixture-secret"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-secret"));
        assert_eq!(fs::read_to_string(&config).unwrap(), original);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}
