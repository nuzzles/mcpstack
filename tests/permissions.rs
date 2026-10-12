//! Shared permission policies must survive adapter round trips without loss.
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(
    client: &str,
    operation: &str,
    config: &Path,
    stack: Option<&Path>,
    flags: &[&str],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcpstack"));
    command.args(["--non-interactive", client, operation]);
    if let Some(stack) = stack {
        command.arg(stack);
    }
    command
        .arg("--config")
        .arg(config)
        .args(flags)
        .env_remove("RUST_LOG")
        .env_remove("NO_COLOR")
        .env_remove("MCPSTACK_COLOR")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn export(client: &str, config: &Path, stack: &Path) -> serde_json::Value {
    let result = run(client, "show", config, None, &["--include-permissions"]);
    assert!(result.status.success(), "{result:?}");
    let value: serde_json::Value = yaml_serde::from_slice(&result.stdout).unwrap();
    let permissions = &value["servers"]["example"]["permissions"];
    assert!(permissions.is_object(), "{value}");
    assert!(permissions.get("source").is_none(), "{value}");
    assert!(!permissions.to_string().contains("mcp__"), "{value}");
    std::fs::write(stack, result.stdout).unwrap();
    value
}

fn apply(client: &str, config: &Path, stack: &Path) {
    let result = run(
        client,
        "use",
        config,
        Some(stack),
        &["--include-permissions", "-y"],
    );
    assert!(result.status.success(), "{result:?}");
}

fn toml(path: &Path) -> toml::Value {
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn json(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
#[cfg(any(unix, windows))]
fn all_codex_approval_modes_and_denials_round_trip_without_source_tags() {
    for default in ["auto", "writes", "approve", "prompt"] {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.toml");
        let target = dir.path().join("target.toml");
        let stack = dir.path().join("stack.yml");
        let original = format!(
            "[mcp_servers.example]\ncommand='tool'\ndefault_tools_approval_mode='{default}'\ndisabled_tools=['delete']\n[mcp_servers.example.tools.read]\napproval_mode='approve'\n[mcp_servers.example.tools.write]\napproval_mode='prompt'\n[mcp_servers.example.tools.automatic]\napproval_mode='auto'\n[mcp_servers.example.tools.mutate]\napproval_mode='writes'\n[mcp_servers.example.tools.delete]\napproval_mode='prompt'\n"
        );
        std::fs::write(&source, original).unwrap();
        let value = export("codex", &source, &stack);
        let policy = &value["servers"]["example"]["permissions"];
        assert_eq!(policy["allow"], serde_json::json!(["read"]));
        assert_eq!(policy["ask"], serde_json::json!(["delete", "write"]));
        assert_eq!(policy["deny"], serde_json::json!(["delete"]));
        assert_eq!(policy["auto"], serde_json::json!(["automatic"]));
        assert_eq!(policy["writes"], serde_json::json!(["mutate"]));
        apply("codex", &target, &stack);
        assert_eq!(toml(&source), toml(&target));
        let before = std::fs::read(&target).unwrap();
        let count = std::fs::read_dir(dir.path()).unwrap().count();
        apply("codex", &target, &stack);
        assert_eq!(std::fs::read(&target).unwrap(), before);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), count);
    }
}

#[test]
#[cfg(any(unix, windows))]
fn claude_patterns_and_overlapping_rules_round_trip_in_the_shared_format() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let settings = dir.path().join("settings.json");
    let target = dir.path().join("restored/claude.json");
    let stack = dir.path().join("stack.yml");
    std::fs::create_dir(target.parent().unwrap()).unwrap();
    std::fs::write(&config, r#"{"mcpServers":{"example":{"command":"tool"}}}"#).unwrap();
    let policy = serde_json::json!({
        "allow": ["mcp__example__*", "mcp__example__read"],
        "ask": ["mcp__example__read", "mcp__example__write*"],
        "deny": ["mcp__example__delete*"]
    });
    std::fs::write(
        &settings,
        serde_json::json!({"permissions": policy}).to_string(),
    )
    .unwrap();
    let value = export("claude", &config, &stack);
    assert_eq!(
        value["servers"]["example"]["permissions"]["ask"],
        serde_json::json!(["read", "write*"])
    );
    apply("claude", &target, &stack);
    assert_eq!(
        json(&target.parent().unwrap().join("settings.json"))["permissions"],
        policy
    );
    let before = std::fs::read(&settings).unwrap();
    apply("claude", &config, &stack);
    assert_eq!(std::fs::read(&settings).unwrap(), before);
    assert!(!dir.path().join("settings.json.~1~").exists());
}

#[test]
#[cfg(any(unix, windows))]
fn shared_rules_round_trip_claude_to_codex_and_back() {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join("claude.json");
    let settings = dir.path().join("settings.json");
    let codex = dir.path().join("config.toml");
    let stack = dir.path().join("stack.yml");
    std::fs::write(&claude, r#"{"mcpServers":{"example":{"command":"tool"}}}"#).unwrap();
    let original = serde_json::json!({
        "allow": ["mcp__example__read"],
        "ask": ["mcp__example__write"],
        "deny": ["mcp__example__delete"]
    });
    std::fs::write(
        &settings,
        serde_json::json!({"permissions": original, "theme": "dark"}).to_string(),
    )
    .unwrap();
    let from_claude = export("claude", &claude, &stack);
    apply("codex", &codex, &stack);
    assert_eq!(
        toml(&codex)["mcp_servers"]["example"]["disabled_tools"][0].as_str(),
        Some("delete")
    );
    let from_codex = export("codex", &codex, &stack);
    assert_eq!(
        from_claude["servers"]["example"]["permissions"],
        from_codex["servers"]["example"]["permissions"]
    );
    std::fs::write(
        &settings,
        r#"{"permissions":{"allow":["Bash"]},"theme":"dark"}"#,
    )
    .unwrap();
    apply("claude", &claude, &stack);
    let mut expected = original;
    expected["allow"]
        .as_array_mut()
        .unwrap()
        .insert(0, "Bash".into());
    assert_eq!(json(&settings)["permissions"], expected);
    assert_eq!(json(&settings)["theme"], "dark");
}

#[test]
#[cfg(any(unix, windows))]
fn supported_defaults_round_trip_codex_to_claude_and_back() {
    for (default, rule) in [("approve", "prompt"), ("prompt", "prompt")] {
        let dir = tempfile::tempdir().unwrap();
        let codex = dir.path().join("source.toml");
        let target = dir.path().join("restored.toml");
        let claude = dir.path().join("claude.json");
        let stack = dir.path().join("stack.yml");
        std::fs::write(&codex, format!("[mcp_servers.example]\ncommand='tool'\ndefault_tools_approval_mode='{default}'\ndisabled_tools=['delete']\n[mcp_servers.example.tools.write]\napproval_mode='{rule}'\n")).unwrap();
        export("codex", &codex, &stack);
        apply("claude", &claude, &stack);
        export("claude", &claude, &stack);
        apply("codex", &target, &stack);
        assert_eq!(toml(&codex), toml(&target));
    }
}

#[test]
fn unsupported_policies_fail_every_preview_and_apply_without_partial_files() {
    let cases = [
        ("claude", "{default_mode: auto}"),
        ("claude", "{writes: [write], allow: [read]}"),
        ("claude", "{default_mode: ask, allow: [read]}"),
        ("codex", "{deny: ['danger*'], allow: [read]}"),
        ("codex", "{ask: ['*'], allow: [read]}"),
        ("codex", "{ask: ['*'], auto: [read]}"),
        ("codex", "{auto: ['*']}"),
        ("codex", "{ask: [read], allow: [read]}"),
    ];
    for (client, policy) in cases {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join(if client == "claude" {
            "claude.json"
        } else {
            "config.toml"
        });
        let settings = dir.path().join("settings.json");
        let stack = dir.path().join("stack.yml");
        let original = if client == "claude" {
            "{\"mcpServers\":{}}\n"
        } else {
            "# preserve\n"
        };
        std::fs::write(&config, original).unwrap();
        std::fs::write(&settings, "{}\n").unwrap();
        std::fs::write(&stack, format!("schema_version: 1\nservers:\n  example:\n    transport: {{type: stdio, command: tool}}\n    permissions: {policy}\n")).unwrap();
        for operation in ["diff", "import", "use"] {
            for dry in [false, true] {
                let mut flags = vec!["--include-permissions"];
                if operation != "diff" {
                    flags.push("-y");
                    if dry {
                        flags.push("--dry-run");
                    }
                }
                let output = run(client, operation, &config, Some(&stack), &flags);
                assert!(!output.status.success(), "{client} {policy}: {output:?}");
                assert!(
                    String::from_utf8_lossy(&output.stderr).contains("without loss"),
                    "{output:?}"
                );
                assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
                assert_eq!(std::fs::read_to_string(&settings).unwrap(), "{}\n");
                assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
            }
        }
    }
}

#[test]
#[cfg(any(unix, windows))]
fn empty_policy_clears_codex_permissions_only_with_explicit_opt_in() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let stack = dir.path().join("stack.yml");
    let original = "[mcp_servers.example]\ncommand='tool'\nenabled_tools=['read']\ndefault_tools_approval_mode='prompt'\ndisabled_tools=['delete']\n[mcp_servers.example.tools.read]\napproval_mode='approve'\n";
    std::fs::write(&config, original).unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers:\n  example:\n    transport: {type: stdio, command: tool}\n    permissions: {}\n").unwrap();
    let ignored = run("codex", "use", &config, Some(&stack), &["-y"]);
    assert!(ignored.status.success(), "{ignored:?}");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    apply("codex", &config, &stack);
    let value = toml(&config);
    let server = value["mcp_servers"]["example"].as_table().unwrap();
    for field in [
        "tools",
        "default_tools_approval_mode",
        "enabled_tools",
        "disabled_tools",
    ] {
        assert!(!server.contains_key(field), "{value}");
    }
}

#[test]
fn legacy_source_tags_and_qualified_rules_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let stack = dir.path().join("stack.yml");
    for permissions in [
        "{source: claude, allow: [read]}",
        "{source: codex, tools: {read: approve}}",
        "{allow: [mcp__example__read]}",
    ] {
        std::fs::write(&stack, format!("schema_version: 1\nservers:\n  example:\n    transport: {{type: stdio, command: tool}}\n    permissions: {permissions}\n")).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
            .arg("validate")
            .arg(&stack)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{output:?}");
    }
}

#[test]
fn permission_export_requires_explicit_opt_in_even_with_all_servers() {
    for client in ["codex", "claude"] {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join(if client == "codex" {
            "config.toml"
        } else {
            "claude.json"
        });
        let original = if client == "codex" {
            "[mcp_servers.example]\ncommand='tool'\nenabled_tools=['read']\ndisabled_tools=['delete']\ndefault_tools_approval_mode='prompt'\n[mcp_servers.example.tools.read]\napproval_mode='approve'\n[mcp_servers.computer-use]\ncommand='computer'\n"
        } else {
            r#"{"mcpServers":{"example":{"command":"tool"},"computer-use":{"command":"computer"}}}"#
        };
        std::fs::write(&config, original).unwrap();
        let settings = dir.path().join("settings.json");
        let rules =
            r#"{"permissions":{"allow":["mcp__example__read"],"deny":["mcp__example__delete"]}}"#;
        std::fs::write(&settings, rules).unwrap();
        for operation in ["show", "export"] {
            for (index, flags) in [
                vec![],
                vec!["--all"],
                vec!["--include-permissions"],
                vec!["--all", "--include-permissions"],
            ]
            .iter()
            .enumerate()
            {
                let file = dir.path().join(format!("{operation}-{index}.yml"));
                let result = run(
                    client,
                    operation,
                    &config,
                    (operation == "export").then_some(file.as_path()),
                    flags,
                );
                assert!(
                    result.status.success(),
                    "{client} {operation} {flags:?}: {result:?}"
                );
                let bytes = if operation == "export" {
                    std::fs::read(&file).unwrap()
                } else {
                    result.stdout
                };
                let stack: serde_json::Value = yaml_serde::from_slice(&bytes).unwrap();
                let server = &stack["servers"]["example"];
                let include = flags.contains(&"--include-permissions");
                assert_eq!(server.get("permissions").is_some(), include, "{stack}");
                assert_eq!(
                    stack["servers"].get("computer-use").is_some(),
                    flags.contains(&"--all")
                );
                assert!(
                    server["settings"].get("disabled_tools").is_none(),
                    "{stack}"
                );
                assert_eq!(
                    server["settings"].get("enabled_tools").is_some(),
                    client == "codex" && include,
                    "{stack}"
                );
                if client == "codex" && include {
                    assert_eq!(
                        server["settings"]["enabled_tools"],
                        serde_json::json!(["read"])
                    );
                    assert_eq!(server["permissions"]["deny"], serde_json::json!(["delete"]));
                }
                assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
                assert_eq!(std::fs::read_to_string(&settings).unwrap(), rules);
            }
        }
    }
}

#[test]
#[cfg(any(unix, windows))]
fn codex_opt_out_preserves_policy_for_legacy_and_portable_stacks() {
    let definitions = [
        "transport: {type: stdio, command: tool}\n    settings: {enabled_tools: [incoming], disabled_tools: [incoming_deny]}\n    permissions: {default_mode: allow, allow: [incoming]}",
        "config:\n      command: tool\n      enabled_tools: [incoming]\n      disabled_tools: [incoming_deny]\n      default_tools_approval_mode: approve\n      tools:\n        read: {approval_mode: approve, output_token_limit: 2000}\n        incoming: {approval_mode: approve}",
    ];
    for (native, definition) in definitions.iter().enumerate() {
        for existing in [false, true] {
            for (operation, flags) in [
                ("diff", vec![]),
                ("import", vec!["-y", "--dry-run"]),
                ("use", vec!["--dry-run"]),
                ("import", vec!["-y"]),
                ("use", vec!["-y"]),
            ] {
                let dir = tempfile::tempdir().unwrap();
                let config = dir.path().join("config.toml");
                let stack = dir.path().join("stack.yml");
                let original = "[mcp_servers.example]\ncommand='old-tool'\nenabled_tools=['read']\ndisabled_tools=['delete']\ndefault_tools_approval_mode='prompt'\n[mcp_servers.example.tools.read]\napproval_mode='prompt'\noutput_token_limit=1000\n[mcp_servers.example.tools.write]\napproval_mode='prompt'\n";
                if existing {
                    std::fs::write(&config, original).unwrap();
                }
                std::fs::write(
                    &stack,
                    format!("schema_version: 1\nservers:\n  example:\n    {definition}\n"),
                )
                .unwrap();
                let result = run("codex", operation, &config, Some(&stack), &flags);
                assert!(result.status.success(), "{operation} {flags:?}: {result:?}");
                let text = String::from_utf8(result.stdout).unwrap();
                assert!(!text.contains("incoming"), "{text}");
                if operation == "diff" || flags.contains(&"--dry-run") {
                    assert_eq!(config.exists(), existing);
                    if existing {
                        assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
                    }
                    assert_eq!(
                        std::fs::read_dir(dir.path()).unwrap().count(),
                        if existing { 2 } else { 1 }
                    );
                    continue;
                }
                let value = toml(&config);
                let server = &value["mcp_servers"]["example"];
                assert_eq!(server["command"].as_str(), Some("tool"));
                if existing {
                    let before: toml::Value = toml::from_str(original).unwrap();
                    for field in [
                        "enabled_tools",
                        "disabled_tools",
                        "default_tools_approval_mode",
                    ] {
                        assert_eq!(server[field], before["mcp_servers"]["example"][field]);
                    }
                    for tool in ["read", "write"] {
                        assert_eq!(
                            server["tools"][tool]["approval_mode"].as_str(),
                            Some("prompt")
                        );
                    }
                } else {
                    for field in [
                        "enabled_tools",
                        "disabled_tools",
                        "default_tools_approval_mode",
                    ] {
                        assert!(server.get(field).is_none(), "{value}");
                    }
                    assert!(
                        server
                            .get("tools")
                            .and_then(|tools| tools.get("read"))
                            .and_then(|tool| tool.get("approval_mode"))
                            .is_none()
                    );
                }
                if native == 1 {
                    assert_eq!(
                        server["tools"]["read"]["output_token_limit"].as_integer(),
                        Some(2000)
                    );
                }
            }
        }
    }
}

#[test]
#[cfg(any(unix, windows))]
fn claude_opt_out_ignores_legacy_filters_and_keeps_settings() {
    for definition in [
        "transport: {type: stdio, command: tool}\n    settings: {enabled_tools: [read], disabled_tools: [delete]}\n    permissions: {default_mode: auto}",
        "config: {command: tool, enabled_tools: [read], disabled_tools: [delete], default_tools_approval_mode: auto, tools: {read: {approval_mode: approve}}}",
    ] {
        for operation in ["diff", "import", "use"] {
            let dir = tempfile::tempdir().unwrap();
            let config = dir.path().join("claude.json");
            let settings = dir.path().join("settings.json");
            let stack = dir.path().join("stack.yml");
            let original = r#"{"permissions":{"deny":["mcp__example__write"]}}"#;
            std::fs::write(&settings, original).unwrap();
            std::fs::write(
                &stack,
                format!("schema_version: 1\nservers:\n  example:\n    {definition}\n"),
            )
            .unwrap();
            let flags: &[&str] = if operation == "diff" { &[] } else { &["-y"] };
            let result = run("claude", operation, &config, Some(&stack), flags);
            assert!(result.status.success(), "{operation}: {result:?}");
            assert_eq!(std::fs::read_to_string(&settings).unwrap(), original);
            assert!(!dir.path().join("settings.json.~1~").exists());
            if operation != "diff" {
                let value = json(&config);
                let server = &value["mcpServers"]["example"];
                assert_eq!(server["command"], "tool");
                assert!(
                    server
                        .as_object()
                        .unwrap()
                        .keys()
                        .all(|key| matches!(key.as_str(), "command" | "type"))
                );
            }
        }
    }
}

#[test]
fn claude_rejects_ambiguous_permission_names_without_writes() {
    for existing in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let codex = dir.path().join("source.toml");
        let config = dir.path().join("claude.json");
        let settings = dir.path().join("settings.json");
        let stack = dir.path().join("stack.yml");
        // Generate the problematic shared stack through the public Codex exporter.
        std::fs::write(&codex, "[mcp_servers.a]\ncommand='tool'\ndisabled_tools=['b__delete']\n[mcp_servers.a__b]\ncommand='tool'\n").unwrap();
        let exported = run("codex", "show", &codex, None, &["--include-permissions"]);
        assert!(exported.status.success(), "{exported:?}");
        std::fs::write(&stack, exported.stdout).unwrap();
        let original = r#"{"mcpServers":{"a__b":{"command":"old-tool"}}}"#;
        let rules = r#"{"permissions":{"deny":["mcp__a__b__delete"]}}"#;
        if existing {
            std::fs::write(&config, original).unwrap();
            std::fs::write(&settings, rules).unwrap();
        }
        let count = std::fs::read_dir(dir.path()).unwrap().count();
        for operation in ["diff", "import", "use"] {
            for dry in [false, true] {
                let mut flags = vec!["--include-permissions"];
                if operation != "diff" {
                    flags.push("-y");
                    if dry {
                        flags.push("--dry-run");
                    }
                }
                // Selection cannot evade collisions with an existing target server.
                if existing && operation != "use" {
                    flags.extend(["--server", "a"]);
                }
                let result = run("claude", operation, &config, Some(&stack), &flags);
                assert!(
                    !result.status.success(),
                    "{operation} {flags:?}: {result:?}"
                );
                assert!(
                    String::from_utf8_lossy(&result.stderr).contains("ambiguous permission rules"),
                    "{result:?}"
                );
                assert!(result.stdout.is_empty(), "{result:?}");
                assert_eq!(config.exists(), existing);
                assert_eq!(settings.exists(), existing);
                if existing {
                    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
                    assert_eq!(std::fs::read_to_string(&settings).unwrap(), rules);
                }
                assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), count);
            }
        }
    }
}

#[test]
fn claude_permission_export_checks_unselected_server_names() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let settings = dir.path().join("settings.json");
    // Include a name between the colliding names in sort order.
    std::fs::write(&config, r#"{"mcpServers":{"a":{"command":"tool"},"a-":{"command":"tool"},"a__b":{"command":"tool"}}}"#).unwrap();
    std::fs::write(
        &settings,
        r#"{"permissions":{"deny":["mcp__a__b__delete"]}}"#,
    )
    .unwrap();
    for operation in ["show", "export"] {
        let file = dir.path().join("stack.yml");
        let flags = ["--include-permissions", "--server", "a"];
        let result = run(
            "claude",
            operation,
            &config,
            (operation == "export").then_some(file.as_path()),
            &flags,
        );
        assert!(!result.status.success(), "{result:?}");
        assert!(String::from_utf8_lossy(&result.stderr).contains("ambiguous permission rules"));
        assert!(result.stdout.is_empty());
        assert!(!file.exists());
    }
    let result = run("claude", "show", &config, None, &["--all"]);
    assert!(result.status.success(), "{result:?}");
    let value: serde_json::Value = yaml_serde::from_slice(&result.stdout).unwrap();
    assert_eq!(value["servers"].as_object().unwrap().len(), 3);
    assert!(value["servers"]["a"].get("permissions").is_none());
}
