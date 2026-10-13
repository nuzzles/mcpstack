use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn command(operation: &str, stack: Option<&Path>, config: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcpstack"));
    command.args([
        "--non-interactive",
        "--no-color",
        "--skip-version-check",
        "claude",
        operation,
    ]);
    if let Some(stack) = stack {
        command.arg(stack);
    }
    command
        .arg("--config")
        .arg(config)
        .env_remove("RUST_LOG")
        .stdin(Stdio::null());
    command
}

fn success(output: Output) -> String {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
#[cfg(any(unix, windows))]
fn claude_stdio_runtime_references_round_trip_without_environment_lookups_or_writes() {
    for runtime_value in [None, Some("unread-fixture-secret\r\nvalue")] {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("claude.json");
        let stack = dir.path().join("stack.yaml");
        let original = json!({"theme":"keep", "mcpServers":{"demo":{
            "command":"example",
            "args":["--token", "${MCPSTACK_AUDIT_RUNTIME_ARG}"],
            "env":{"API_TOKEN":"${MCPSTACK_AUDIT_RUNTIME_TOKEN}"}
        }}})
        .to_string();
        fs::write(&config, &original).unwrap();
        let configure = |command: &mut Command| {
            for name in ["MCPSTACK_AUDIT_RUNTIME_ARG", "MCPSTACK_AUDIT_RUNTIME_TOKEN"] {
                if let Some(value) = runtime_value {
                    command.env(name, value);
                } else {
                    command.env_remove(name);
                }
            }
        };
        let mut export = command("export", Some(&stack), &config);
        configure(&mut export);
        success(export.output().unwrap());
        let yaml = fs::read_to_string(&stack).unwrap();
        assert!(yaml.contains("env: MCPSTACK_AUDIT_RUNTIME_ARG"));
        assert!(yaml.contains("env: MCPSTACK_AUDIT_RUNTIME_TOKEN"));
        assert!(!yaml.contains("value_env"));
        assert!(!yaml.contains("fixture-secret"));
        for (operation, flags) in [
            ("diff", vec![]),
            ("import", vec!["--dry-run", "-y"]),
            ("use", vec!["--dry-run"]),
            ("import", vec!["-y"]),
            ("use", vec!["-y"]),
        ] {
            let mut apply = command(operation, Some(&stack), &config);
            configure(&mut apply);
            let output = success(apply.args(flags).output().unwrap());
            assert!(output.contains("No changes"), "{output}");
            assert!(!output.contains("fixture-secret"));
            assert_eq!(fs::read_to_string(&config).unwrap(), original);
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        }
    }
}

#[test]
#[cfg(any(unix, windows))]
fn claude_stdio_distinguishes_runtime_and_masked_literal_references() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yaml");
    fs::write(&config, "{}").unwrap();
    fs::write(
        &stack,
        json!({"schema_version":1,"servers":{"demo":{"transport":{
            "type":"stdio", "command":"example",
            "args":[{"env":"MCPSTACK_AUDIT_RUNTIME_ARG"},
                {"env":"MCPSTACK_AUDIT_RUNTIME_ARG","prefix":"Bearer "},
                {"value_env":"MCPSTACK_AUDIT_LITERAL"}],
            "env":{"RUNTIME":{"env":"MCPSTACK_AUDIT_RUNTIME_ARG"},
                "PREFIX":{"env":"MCPSTACK_AUDIT_RUNTIME_ARG","prefix":"Bearer "},
                "LITERAL":{"value_env":"MCPSTACK_AUDIT_LITERAL"}}
        }}}})
        .to_string(),
    )
    .unwrap();
    let missing = command("import", Some(&stack), &config)
        .arg("-y")
        .env_remove("MCPSTACK_AUDIT_RUNTIME_ARG")
        .env_remove("MCPSTACK_AUDIT_LITERAL")
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert_eq!(fs::read_to_string(&config).unwrap(), "{}");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    for operation in ["diff", "import"] {
        let mut run = command(operation, Some(&stack), &config);
        if operation == "import" {
            run.arg("-y");
        }
        let text = success(
            run.env_remove("MCPSTACK_AUDIT_RUNTIME_ARG")
                .env("MCPSTACK_AUDIT_LITERAL", "resolved-fixture-secret")
                .output()
                .unwrap(),
        );
        assert!(!text.contains("resolved-fixture-secret"), "{text}");
    }
    let actual: Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    assert_eq!(
        actual["mcpServers"]["demo"]["args"],
        json!([
            "${MCPSTACK_AUDIT_RUNTIME_ARG}",
            "Bearer ${MCPSTACK_AUDIT_RUNTIME_ARG}",
            "resolved-fixture-secret"
        ])
    );
    assert_eq!(
        actual["mcpServers"]["demo"]["env"],
        json!({
            "RUNTIME":"${MCPSTACK_AUDIT_RUNTIME_ARG}",
            "PREFIX":"Bearer ${MCPSTACK_AUDIT_RUNTIME_ARG}",
            "LITERAL":"resolved-fixture-secret"
        })
    );
}

#[test]
fn claude_rejects_invalid_native_fields_before_previews_exports_or_writes() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yaml");
    let exported = dir.path().join("export.yaml");
    let original = r#"{"theme":"keep","mcpServers":{"keep":{"command":"example"}}}"#;
    let mut invalid = vec![
        json!({"command":"  "}),
        json!({"command":"example\nprivate-fixture"}),
        json!({"command":"example","args":["private-fixture\u{0}"]}),
        json!({"command":"example","env":{"BAD NAME":"private-fixture"}}),
        json!({"command":"example","env":{"KEY":"private-fixture\u{0}"}}),
    ];
    for kind in ["http", "sse"] {
        for url in [
            "private-fixture",
            "file:///private-fixture",
            "wss://example.com/private-fixture",
            "https://user:private-fixture@example.com",
            "https://example.com/#private-fixture",
        ] {
            invalid.push(json!({"type":kind,"url":url}));
        }
        for headers in [
            json!({"Bad Header":"private-fixture"}),
            json!({"Authorization":"private-fixture\r\nX-Invalid: value"}),
            json!({"Authorization":"private-fixture", "authorization":"duplicate"}),
        ] {
            invalid.push(json!({"type":kind,"url":"https://example.com/mcp","headers":headers}));
        }
    }
    for definition in invalid {
        fs::write(&config, original).unwrap();
        fs::write(
            &stack,
            json!({"schema_version":1,"servers":{
                "valid":{"transport":{"type":"stdio","command":"example"}},
                "invalid":{"config":definition}
            }})
            .to_string(),
        )
        .unwrap();
        for (operation, flags) in [
            ("diff", vec![]),
            ("import", vec!["--dry-run", "-y"]),
            ("use", vec!["--dry-run"]),
            ("import", vec!["-y"]),
            ("use", vec!["-y"]),
        ] {
            let output = command(operation, Some(&stack), &config)
                .args(flags)
                .output()
                .unwrap();
            assert!(!output.status.success(), "{definition}: {output:?}");
            assert!(output.stdout.is_empty(), "{output:?}");
            assert!(!String::from_utf8_lossy(&output.stderr).contains("private-fixture"));
            assert_eq!(fs::read_to_string(&config).unwrap(), original);
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        }
        let native = json!({"mcpServers":{"invalid":definition}}).to_string();
        fs::write(&config, &native).unwrap();
        for (operation, path) in [("show", None), ("export", Some(exported.as_path()))] {
            let output = command(operation, path, &config).output().unwrap();
            assert!(!output.status.success(), "{definition}: {output:?}");
            assert!(output.stdout.is_empty());
            assert!(!String::from_utf8_lossy(&output.stderr).contains("private-fixture"));
            assert_eq!(fs::read_to_string(&config).unwrap(), native);
            assert!(!exported.exists());
        }
    }
}

#[test]
fn claude_validates_resolved_http_and_sse_headers_before_any_changes() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let settings = dir.path().join("settings.json");
    let stack = dir.path().join("stack.yaml");
    let original = r#"{"mcpServers":{},"theme":"keep"}"#;
    let policy = r#"{"permissions":{"deny":["mcp__demo__write"]}}"#;
    fs::write(&config, original).unwrap();
    fs::write(&settings, policy).unwrap();
    for kind in ["http", "sse"] {
        for native in [false, true] {
            let definition = if native {
                json!({"config":{"type":kind,"url":"https://example.com/mcp",
                    "headers":{"Authorization":{"$env":"MCPSTACK_AUDIT_BAD_HEADER"}}}})
            } else {
                json!({"transport":{"type":kind,"url":"https://example.com/mcp",
                    "headers":{"Authorization":{"value_env":"MCPSTACK_AUDIT_BAD_HEADER"}}},
                    "permissions":{"allow":["write"]}})
            };
            fs::write(
                &stack,
                json!({"schema_version":1,"servers":{"demo":definition}}).to_string(),
            )
            .unwrap();
            for value in [
                "private-fixture\r\nX-Invalid: value",
                "private-fixture\tvalue",
            ] {
                for (operation, flags) in [
                    ("diff", vec![]),
                    ("import", vec!["--dry-run", "-y"]),
                    ("use", vec!["--dry-run"]),
                    ("import", vec!["-y"]),
                    ("use", vec!["-y"]),
                ] {
                    let output = command(operation, Some(&stack), &config)
                        .args(flags)
                        .arg("--include-permissions")
                        .env("MCPSTACK_AUDIT_BAD_HEADER", value)
                        .output()
                        .unwrap();
                    assert!(!output.status.success(), "{output:?}");
                    assert!(output.stdout.is_empty(), "{output:?}");
                    assert!(!String::from_utf8_lossy(&output.stderr).contains("private-fixture"));
                    assert_eq!(fs::read_to_string(&config).unwrap(), original);
                    assert_eq!(fs::read_to_string(&settings).unwrap(), policy);
                    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
                }
            }
        }
    }
}
