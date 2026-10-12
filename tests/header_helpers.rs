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
    variables: &[(&str, &str)],
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
        .env_remove("MCPSTACK_HELPER_COMMAND")
        .env("MCPSTACK_HELPER_RUNTIME_TOKEN", "runtime-fixture-secret")
        .envs(variables.iter().copied())
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn success(output: Output) -> String {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

fn helper_key(harness: &str) -> &'static str {
    if harness == "claude" {
        "headersHelper"
    } else {
        "http_headers_helper"
    }
}

fn server(helper: &str, harness: &str) -> Value {
    if harness == "claude" {
        json!({"type":"http", "url":"https://example.com/mcp", "headersHelper":helper,
            "headers":{"Authorization":"Bearer ${MCPSTACK_HELPER_RUNTIME_TOKEN}", "X-Client":"work"}})
    } else {
        json!({"url":"https://example.com/mcp", "http_headers_helper":helper,
            "bearer_token_env_var":"MCPSTACK_HELPER_RUNTIME_TOKEN", "http_headers":{"X-Client":"work"}})
    }
}

fn write_config(config: &Path, harness: &str, servers: Value) {
    let text = if harness == "claude" {
        json!({"theme":"dark", "mcpServers":servers}).to_string()
    } else {
        toml::to_string(&json!({"model":"example", "mcp_servers":servers})).unwrap()
    };
    fs::write(config, text).unwrap();
}

fn read_servers(config: &Path, harness: &str) -> Value {
    let text = fs::read_to_string(config).unwrap();
    let (value, key): (Value, _) = if harness == "claude" {
        (serde_json::from_str(&text).unwrap(), "mcpServers")
    } else {
        (
            serde_json::to_value(toml::from_str::<toml::Value>(&text).unwrap()).unwrap(),
            "mcp_servers",
        )
    };
    value[key].clone()
}

#[test]
fn helpers_round_trip_between_harnesses_with_native_auth_inputs_without_execution() {
    for origin in ["codex", "claude"] {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("executed");
        let helper = format!("echo executed > \"{}\"", marker.display());
        let source = dir.path().join("source.config");
        let stack = dir.path().join("stack.yml");
        write_config(&source, origin, json!({"events":server(&helper, origin)}));
        let original = fs::read(&source).unwrap();
        success(run(origin, "export", Some(&stack), &source, &[], &[]));
        let portable: Value = yaml_serde::from_str(&fs::read_to_string(&stack).unwrap()).unwrap();
        let transport = &portable["servers"]["events"]["transport"];
        assert_eq!(transport["headers_helper"], helper);
        assert_eq!(
            transport["headers"]["Authorization"],
            json!({"env":"MCPSTACK_HELPER_RUNTIME_TOKEN", "prefix":"Bearer "})
        );
        assert_eq!(transport["headers"]["X-Client"], "work");
        assert_eq!(fs::read(&source).unwrap(), original);
        for target in ["codex", "claude"] {
            let config = dir.path().join(format!("{target}.config"));
            write_config(&config, target, json!({"old":{"command":"old-tool"}}));
            let before = fs::read(&config).unwrap();
            for (operation, flags) in [
                ("diff", vec![]),
                ("import", vec!["-y", "--dry-run"]),
                ("use", vec!["--dry-run"]),
            ] {
                let preview = success(run(target, operation, Some(&stack), &config, &flags, &[]));
                assert!(!preview.contains("runtime-fixture-secret"));
                assert!(preview.contains("events"));
                assert_eq!(fs::read(&config).unwrap(), before);
                assert!(!marker.exists());
            }
            success(run(target, "import", Some(&stack), &config, &["-y"], &[]));
            assert_eq!(
                read_servers(&config, target)["events"],
                server(&helper, target)
            );
            assert!(read_servers(&config, target).get("old").is_some());
            assert_eq!(
                fs::read(dir.path().join(format!("{target}.config.~1~"))).unwrap(),
                before
            );
            success(run(target, "use", Some(&stack), &config, &["-y"], &[]));
            assert!(read_servers(&config, target).get("old").is_none());
            let before = fs::read(&config).unwrap();
            for operation in ["diff", "import", "use"] {
                let output = success(run(
                    target,
                    operation,
                    Some(&stack),
                    &config,
                    if operation == "diff" { &[] } else { &["-y"] },
                    &[],
                ));
                assert!(output.contains("No changes"), "{output}");
                assert_eq!(fs::read(&config).unwrap(), before);
                assert!(!dir.path().join(format!("{target}.config.~3~")).exists());
            }
            let shown = success(run(target, "show", None, &config, &[], &[]));
            let shown: Value = yaml_serde::from_str(&shown).unwrap();
            assert_eq!(shown, portable);
            success(run(target, "list", None, &config, &[], &[]));
            assert!(!marker.exists());
        }
    }
}

#[test]
fn claude_sse_and_websocket_helpers_preserve_shell_syntax_and_static_headers() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    let command = "${HELPER_PROGRAM} --profile work\nget-token | format-headers";
    for (kind, url) in [
        ("sse", "https://example.com/sse"),
        ("ws", "wss://example.com/events"),
        ("ws", "ws://localhost:9/events"),
    ] {
        write_config(
            &config,
            "claude",
            json!({"events":{"type":kind,"url":url,"headersHelper":command,"headers":{"Authorization":"literal-fixture-secret","X-Client":"work"}}}),
        );
        let before = fs::read(&config).unwrap();
        success(run(
            "claude",
            "export",
            Some(&stack),
            &config,
            &["--expose-secrets"],
            &[],
        ));
        let exported: Value = yaml_serde::from_str(&fs::read_to_string(&stack).unwrap()).unwrap();
        assert_eq!(
            exported["servers"]["events"]["transport"]["headers_helper"],
            command
        );
        for operation in ["diff", "import", "use"] {
            let result = success(run(
                "claude",
                operation,
                Some(&stack),
                &config,
                if operation == "diff" { &[] } else { &["-y"] },
                &[],
            ));
            assert!(result.contains("No changes"));
            assert!(!result.contains("literal-fixture-secret"));
            assert_eq!(fs::read(&config).unwrap(), before);
        }
        // Exports require a fresh destination.
        fs::remove_file(&stack).unwrap();
    }
}

#[test]
fn credential_bearing_helpers_are_masked_and_restored_as_whole_commands() {
    for origin in ["codex", "claude"] {
        for command in [
            "auth-tool --token inline-fixture-secret",
            "auth-tool --api-key=inline-fixture-secret",
            "TOKEN=inline-fixture-secret auth-tool",
            "echo '{\"Authorization\":\"Bearer inline-fixture-secret\"}'",
            r#"echo "{\"Authorization\":\"Bearer inline-fixture-secret\"}""#,
            "auth-tool -H 'Cookie: session=inline-fixture-secret'",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let config = dir.path().join("source.config");
            let stack = dir.path().join("stack.yml");
            write_config(&config, origin, json!({"events":server(command, origin)}));
            let shown = success(run(origin, "show", None, &config, &[], &[]));
            assert!(!shown.contains("inline-fixture-secret"));
            let masked: Value = yaml_serde::from_str(&shown).unwrap();
            let variable = masked["servers"]["events"]["transport"]["headers_helper"]["value_env"]
                .as_str()
                .unwrap();
            fs::write(&stack, shown.as_bytes()).unwrap();
            let exposed = success(run(
                origin,
                "show",
                None,
                &config,
                &["--expose-secrets"],
                &[],
            ));
            let exposed: Value = yaml_serde::from_str(&exposed).unwrap();
            assert_eq!(
                exposed["servers"]["events"]["transport"]["headers_helper"],
                command
            );
            for target in ["claude", "codex"] {
                let target_config = dir.path().join(format!("{target}.config"));
                // Both the replaced value and the new value must be redacted.
                write_config(
                    &target_config,
                    target,
                    json!({"events":server("auth-tool --token old-fixture-secret", target)}),
                );
                let preview = success(run(
                    target,
                    "diff",
                    Some(&stack),
                    &target_config,
                    &[],
                    &[(variable, command)],
                ));
                assert!(!preview.contains("inline-fixture-secret"));
                assert!(!preview.contains("old-fixture-secret"));
                assert!(preview.contains("redacted"));
                success(run(
                    target,
                    "import",
                    Some(&stack),
                    &target_config,
                    &["-y"],
                    &[(variable, command)],
                ));
                assert_eq!(
                    read_servers(&target_config, target)["events"][helper_key(target)],
                    command
                );
                assert!(
                    !success(run(target, "show", None, &target_config, &[], &[]))
                        .contains("inline-fixture-secret")
                );
            }
        }
    }
}

#[test]
fn ordinary_helper_commands_remain_literal() {
    let dir = tempfile::tempdir().unwrap();
    for harness in ["claude", "codex"] {
        let config = dir.path().join(format!("{harness}.config"));
        for command in [
            "get-token",
            "./scripts/get-token.sh",
            "auth-tool --profile work",
            "${AUTH_HELPER}",
            "\"C:\\Program Files\\Auth\\helper.exe\" --format json",
        ] {
            write_config(&config, harness, json!({"events":server(command, harness)}));
            let shown = success(run(harness, "show", None, &config, &[], &[]));
            let shown: Value = yaml_serde::from_str(&shown).unwrap();
            assert_eq!(
                shown["servers"]["events"]["transport"]["headers_helper"],
                command
            );
        }
    }
}

#[test]
fn invalid_helpers_fail_without_config_changes_or_export_files() {
    for harness in ["claude", "codex"] {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let stack = dir.path().join("stack.yml");
        let exported = dir.path().join("export.yml");
        write_config(&config, harness, json!({"keep":{"command":"tool"}}));
        let original = fs::read(&config).unwrap();
        let mut invalid = vec![
            json!(""),
            json!(" \n\t"),
            json!("bad\0fixture-secret"),
            json!(42),
            json!({"bad":"fixture-secret"}),
        ]
        .into_iter()
        .map(|helper| {
            let mut native = server("placeholder", harness);
            native[helper_key(harness)] = helper;
            native
        })
        .collect::<Vec<_>>();
        invalid.push(json!({"command":"tool", helper_key(harness):"fixture-secret"}));
        if harness == "claude" {
            invalid.push(json!({"url":"https://example.com/mcp", "type":"http", "headersHelper":"one", "http_headers_helper":"fixture-secret"}));
        }
        if harness == "codex" {
            let mut remote = server("fixture-secret", harness);
            remote["environment_id"] = json!("remote");
            invalid.push(remote);
        }
        for definition in invalid {
            fs::write(&config, &original).unwrap();
            fs::write(&stack, json!({"schema_version":1,"servers":{"bad":{"config":definition},"valid":{"transport":{"type":"stdio","command":"tool"}}}}).to_string()).unwrap();
            for operation in ["diff", "import", "use"] {
                let output = run(
                    harness,
                    operation,
                    Some(&stack),
                    &config,
                    if operation == "diff" { &[] } else { &["-y"] },
                    &[],
                );
                assert!(!output.status.success(), "{definition}: {output:?}");
                assert!(!String::from_utf8_lossy(&output.stdout).contains("fixture-secret"));
                assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-secret"));
                assert_eq!(fs::read(&config).unwrap(), original);
                assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
            }
            write_config(&config, harness, json!({"bad":definition}));
            let output = run(harness, "export", Some(&exported), &config, &[], &[]);
            assert!(!output.status.success(), "{output:?}");
            assert!(output.stdout.is_empty());
            assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-secret"));
            assert!(!exported.exists());
        }
    }
}

#[test]
fn masked_helper_values_are_validated_after_resolution_and_never_run() {
    let dir = tempfile::tempdir().unwrap();
    let stack = dir.path().join("stack.yml");
    fs::write(&stack, "schema_version: 1\nservers:\n  events:\n    transport:\n      type: http\n      url: https://example.com/mcp\n      headers_helper: {value_env: MCPSTACK_HELPER_COMMAND}\n").unwrap();
    for harness in ["claude", "codex"] {
        let config = dir.path().join(format!("{harness}.config"));
        write_config(&config, harness, json!({}));
        let original = fs::read(&config).unwrap();
        for value in [None, Some(""), Some(" \n\t")] {
            let variables: Vec<_> = value
                .map(|value| ("MCPSTACK_HELPER_COMMAND", value))
                .into_iter()
                .collect();
            for operation in ["import", "use"] {
                let result = run(
                    harness,
                    operation,
                    Some(&stack),
                    &config,
                    &["-y"],
                    &variables,
                );
                assert!(!result.status.success(), "{result:?}");
                assert_eq!(fs::read(&config).unwrap(), original);
                assert!(!dir.path().join(format!("{harness}.config.~1~")).exists());
            }
        }
        let marker = dir.path().join("executed");
        let command = format!("echo executed > \"{}\"", marker.display());
        success(run(
            harness,
            "use",
            Some(&stack),
            &config,
            &["-y"],
            &[("MCPSTACK_HELPER_COMMAND", &command)],
        ));
        assert_eq!(
            read_servers(&config, harness)["events"][helper_key(harness)],
            command
        );
        assert!(!marker.exists());
    }
}

#[test]
fn codex_native_helper_can_coexist_with_oauth_settings() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let stack = dir.path().join("stack.yml");
    let native = json!({"url":"https://example.com/mcp", "http_headers_helper":"auth-tool --profile work", "auth":"oauth", "oauth":{"client_id":"public-client", "callback_port":1234}, "scopes":["read"]});
    fs::write(
        &stack,
        json!({"schema_version":1,"servers":{"events":{"config":native}}}).to_string(),
    )
    .unwrap();
    write_config(&config, "codex", json!({}));
    success(run("codex", "use", Some(&stack), &config, &["-y"], &[]));
    assert_eq!(read_servers(&config, "codex")["events"], native);
    assert!(success(run("codex", "diff", Some(&stack), &config, &[], &[])).contains("No changes"));
}

#[test]
fn native_helper_references_restore_commands_for_both_targets() {
    let dir = tempfile::tempdir().unwrap();
    let stack = dir.path().join("stack.yml");
    fs::write(&stack, json!({"schema_version":1,"servers":{"events":{"config":{
        "url":"https://example.com/mcp", "http_headers_helper":{"$env":"MCPSTACK_HELPER_COMMAND"}
    }}}}).to_string()).unwrap();
    let marker = dir.path().join("executed");
    let command = format!("echo executed > \"{}\"", marker.display());
    for harness in ["codex", "claude"] {
        let config = dir.path().join(format!("{harness}.config"));
        write_config(&config, harness, json!({}));
        success(run(
            harness,
            "import",
            Some(&stack),
            &config,
            &["-y"],
            &[("MCPSTACK_HELPER_COMMAND", &command)],
        ));
        assert_eq!(
            read_servers(&config, harness)["events"][helper_key(harness)],
            command
        );
        assert!(!marker.exists());
    }
}
