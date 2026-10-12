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
    let original = "[mcp_servers.example]\ncommand='tool'\ndefault_tools_approval_mode='prompt'\ndisabled_tools=['delete']\n[mcp_servers.example.tools.read]\napproval_mode='approve'\n";
    std::fs::write(&config, original).unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers:\n  example:\n    transport: {type: stdio, command: tool}\n    permissions: {}\n").unwrap();
    let ignored = run("codex", "use", &config, Some(&stack), &["-y"]);
    assert!(ignored.status.success(), "{ignored:?}");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    apply("codex", &config, &stack);
    let value = toml(&config);
    let server = value["mcp_servers"]["example"].as_table().unwrap();
    for field in ["tools", "default_tools_approval_mode", "disabled_tools"] {
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
