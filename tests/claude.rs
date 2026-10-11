use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(operation: &str, stack: Option<&Path>, config: &Path, flags: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcpstack"));
    command.args(["--non-interactive", "claude", operation]);
    if let Some(stack) = stack {
        command.arg(stack);
    }
    command
        .arg("--config")
        .arg(config)
        .args(flags)
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
#[cfg(any(unix, windows))]
fn user_config_round_trip_and_switch_preserve_unrelated_settings() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    let original = r#"{"theme":"dark","projects":{"demo":{"mcpServers":{"local":{"command":"other"}}}},"mcpServers":{"old":{"command":"old"}}}"#;
    std::fs::write(&config, original).unwrap();
    std::fs::write(&stack,"schema_version: 1\nservers:\n  api:\n    transport: {type: http, url: 'https://example.com/mcp'}\n  process:\n    config: {command: tool, args: ['--quiet']}\n").unwrap();
    let preview = run("use", Some(&stack), &config, &["--dry-run"]);
    assert!(preview.status.success(), "{preview:?}");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    assert!(!dir.path().join("claude.json.~1~").exists());
    let applied = run("use", Some(&stack), &config, &["-y"]);
    assert!(applied.status.success(), "{applied:?}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("claude.json.~1~")).unwrap(),
        original
    );
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
    assert_eq!(value["theme"], "dark");
    assert_eq!(
        value["projects"]["demo"]["mcpServers"]["local"]["command"],
        "other"
    );
    assert!(value["mcpServers"].get("old").is_none());
    assert_eq!(value["mcpServers"]["api"]["type"], "http");
    let export = run("export", None, &config, &[]);
    assert!(export.status.success(), "{export:?}");
    let exported = String::from_utf8(export.stdout).unwrap();
    assert!(exported.contains("process:"));
    assert!(exported.contains("api:"));
    let bytes = std::fs::read(&config).unwrap();
    assert!(run("use", Some(&stack), &config, &["-y"]).status.success());
    assert_eq!(std::fs::read(&config).unwrap(), bytes);
    assert!(!dir.path().join("claude.json.~2~").exists());
}

#[test]
#[cfg(any(unix, windows))]
fn masked_export_and_invalid_import_do_not_expose_or_write_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    std::fs::write(&config,r#"{"mcpServers":{"api":{"type":"http","url":"https://example.com/mcp","headers":{"Authorization":"Bearer fixture-secret"}}}}"#).unwrap();
    let export = run("export", None, &config, &[]);
    assert!(export.status.success(), "{export:?}");
    let yaml = String::from_utf8(export.stdout).unwrap();
    assert!(!yaml.contains("fixture-secret"));
    assert!(yaml.contains("env: MCPSTACK_"));
    std::fs::write(
        &stack,
        "schema_version: 1\nservers:\n  bad:\n    config: {command: tool, codex_only: true}\n",
    )
    .unwrap();
    let before = std::fs::read(&config).unwrap();
    let import = run("import", Some(&stack), &config, &["-y"]);
    assert!(!import.status.success());
    assert_eq!(std::fs::read(&config).unwrap(), before);
    assert!(!dir.path().join("claude.json.~1~").exists());
}

#[test]
fn duplicate_json_keys_and_missing_secret_references_fail_without_writes() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    let duplicate = r#"{"mcpServers":{"a":{"command":"first"},"a":{"command":"second"}}}"#;
    std::fs::write(&config, duplicate).unwrap();
    let export = run("export", None, &config, &[]);
    assert!(!export.status.success());
    assert!(export.stdout.is_empty());
    std::fs::write(&config, r#"{"mcpServers":{},"theme":"dark"}"#).unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers:\n  api:\n    config:\n      type: http\n      url: https://example.com/mcp\n      headers:\n        Authorization: {'$env': MCPSTACK_CLAUDE_TEST_MISSING_SECRET}\n").unwrap();
    let before = std::fs::read(&config).unwrap();
    let result = run("use", Some(&stack), &config, &["-y"]);
    assert!(!result.status.success());
    assert_eq!(std::fs::read(&config).unwrap(), before);
    assert!(!dir.path().join("claude.json.~1~").exists());
}

#[test]
#[cfg(any(unix, windows))]
fn import_merges_without_removing_servers_and_diff_redacts_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    let original = r#"{"theme":"dark","mcpServers":{"old":{"command":"old"},"api":{"type":"http","url":"https://example.com/old","headers":{"Authorization":"Bearer old-secret"}}}}"#;
    std::fs::write(&config, original).unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers:\n  api:\n    config:\n      type: http\n      url: https://example.com/new\n      headers: {Authorization: 'Bearer new-secret'}\n").unwrap();
    let diff = run("diff", Some(&stack), &config, &[]);
    assert!(diff.status.success(), "{diff:?}");
    let text = String::from_utf8(diff.stdout).unwrap();
    assert!(text.contains("https://example.com/old"));
    assert!(text.contains("https://example.com/new"));
    assert!(!text.contains("old-secret"));
    assert!(!text.contains("new-secret"));
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    let applied = run("import", Some(&stack), &config, &["-y"]);
    assert!(applied.status.success(), "{applied:?}");
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
    assert_eq!(value["mcpServers"]["old"]["command"], "old");
    assert_eq!(value["mcpServers"]["api"]["url"], "https://example.com/new");
    assert_eq!(value["theme"], "dark");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("claude.json.~1~")).unwrap(),
        original
    );
}

#[test]
fn credential_only_changes_remain_visible_in_both_previews() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    let original = r#"{"mcpServers":{"api":{"type":"http","url":"https://example.com/mcp","headers":{"Authorization":"Bearer old-fixture"}}}}"#;
    std::fs::write(&config, original).unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers:\n  api:\n    config:\n      type: http\n      url: https://example.com/mcp\n      headers: {Authorization: 'Bearer new-fixture'}\n").unwrap();
    for (operation, flags) in [("diff", vec![]), ("import", vec!["--dry-run", "-y"])] {
        let output = run(operation, Some(&stack), &config, &flags);
        assert!(output.status.success(), "{output:?}");
        let preview = String::from_utf8(output.stdout).unwrap();
        assert!(preview.contains("<redacted: changed>"), "{preview}");
        assert!(!preview.contains("old-fixture"));
        assert!(!preview.contains("new-fixture"));
    }
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    assert!(!dir.path().join("claude.json.~1~").exists());
}

#[test]
fn non_string_server_type_is_rejected_before_a_backup_or_write() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    let original = r#"{"theme":"dark","mcpServers":{}}"#;
    std::fs::write(&config, original).unwrap();
    std::fs::write(
        &stack,
        "schema_version: 1\nservers:\n  bad:\n    config: {type: 17, command: tool}\n",
    )
    .unwrap();
    for operation in ["diff", "import", "use"] {
        let flags = if operation == "diff" {
            vec![]
        } else {
            vec!["-y"]
        };
        let output = run(operation, Some(&stack), &config, &flags);
        assert!(!output.status.success(), "{operation}: {output:?}");
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
        assert!(!dir.path().join("claude.json.~1~").exists());
    }
}

#[test]
fn unsupported_stack_reports_all_server_fields_without_values_or_writes() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    let original = r#"{"mcpServers":{}}"#;
    std::fs::write(&config, original).unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers:\n  computer-use:\n    config: {command: tool, cwd: '/private/fixture-secret', enabled: true}\n  github:\n    config: {url: 'https://example.com/mcp', bearer_token_env_var: PRIVATE_TOKEN}\n  remote:\n    transport: {type: http, url: 'https://example.com/mcp', headers: {Authorization: {env: PRIVATE_TOKEN, prefix: 'Bearer '}}}\n").unwrap();
    let output = run("use", Some(&stack), &config, &["--dry-run"]);
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    for field in ["cwd", "enabled", "reserved by Claude Code"] {
        assert!(error.contains(field), "missing {field}: {error}");
    }
    assert!(!error.contains("bearer_token_env_var"));
    assert!(!error.contains("transport.bearer_token"));
    assert!(!error.contains("fixture-secret"));
    assert!(!error.contains("PRIVATE_TOKEN"));
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    assert!(!dir.path().join("claude.json.~1~").exists());
}

#[test]
fn reserved_claude_server_names_are_rejected_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    let original = r#"{"mcpServers":{}}"#;
    std::fs::write(&config, original).unwrap();
    std::fs::write(
        &stack,
        "schema_version: 1\nservers:\n  computer-use:\n    config: {command: tool}\n",
    )
    .unwrap();
    let output = run("use", Some(&stack), &config, &["-y"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("reserved by Claude Code")
    );
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    assert!(!dir.path().join("claude.json.~1~").exists());
}

#[test]
fn export_skips_reserved_server_names_before_validation() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let original = r#"{"mcpServers":{"computer-use":{"unexpected":"fixture"},"Claude Preview":{"future":true},"ordinary":{"command":"tool"}}}"#;
    std::fs::write(&config, original).unwrap();
    let result = run("export", None, &config, &[]);
    assert!(result.status.success(), "{result:?}");
    let output = String::from_utf8(result.stdout).unwrap();
    assert!(output.contains("ordinary"));
    assert!(!output.contains("computer-use"));
    assert!(!output.contains("Claude Preview"));
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
}

#[test]
fn export_allow_flags_include_only_named_servers() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let names = [
        ("workspace", "--allow-workspace"),
        ("claude-in-chrome", "--allow-claude-in-chrome"),
        ("computer-use", "--allow-computer-use"),
        ("Claude Preview", "--allow-claude-preview"),
        ("Claude Browser", "--allow-claude-browser"),
        ("node_repl", "--allow-node-repl"),
    ];
    let servers: serde_json::Map<String, serde_json::Value> = names
        .iter()
        .map(|(name, _)| ((*name).into(), serde_json::json!({"command":"tool"})))
        .collect();
    std::fs::write(
        &config,
        serde_json::json!({"mcpServers":servers}).to_string(),
    )
    .unwrap();
    let default = run("export", None, &config, &[]);
    assert!(default.status.success(), "{default:?}");
    let stack: serde_json::Value = yaml_serde::from_slice(&default.stdout).unwrap();
    assert!(stack["servers"].as_object().unwrap().is_empty());
    for (name, flag) in names {
        let result = run("export", None, &config, &[flag]);
        assert!(result.status.success(), "{result:?}");
        let stack: serde_json::Value = yaml_serde::from_slice(&result.stdout).unwrap();
        let exported = stack["servers"].as_object().unwrap();
        assert_eq!(exported.len(), 1);
        assert!(exported.contains_key(name));
    }
    let all = run("export", None, &config, &["--all"]);
    assert!(all.status.success(), "{all:?}");
    let stack: serde_json::Value = yaml_serde::from_slice(&all.stdout).unwrap();
    assert_eq!(stack["servers"].as_object().unwrap().len(), names.len());
}

#[test]
#[cfg(any(unix, windows))]
fn codex_http_headers_and_runtime_bindings_map_to_claude_headers() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    std::fs::write(&config, r#"{"mcpServers":{},"theme":"dark"}"#).unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers:\n  direct:\n    config: {url: 'https://example.com/mcp', http_headers: {X-Test: fixture-value}}\n  bearer:\n    config: {url: 'https://example.com/mcp', bearer_token_env_var: SERVICE_TOKEN}\n  environment:\n    config: {url: 'https://example.com/mcp', env_http_headers: {X-API-Key: SERVICE_KEY}}\n  portable:\n    transport: {type: http, url: 'https://example.com/mcp', headers: {Authorization: {env: PORTABLE_TOKEN, prefix: 'Bearer '}}}\n").unwrap();
    let output = run("use", Some(&stack), &config, &["-y"]);
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
    let servers = &value["mcpServers"];
    for name in ["direct", "bearer", "environment", "portable"] {
        assert_eq!(servers[name]["type"], "http");
    }
    assert_eq!(servers["direct"]["headers"]["X-Test"], "fixture-value");
    assert_eq!(
        servers["bearer"]["headers"]["Authorization"],
        "Bearer ${SERVICE_TOKEN}"
    );
    assert_eq!(
        servers["environment"]["headers"]["X-API-Key"],
        "${SERVICE_KEY}"
    );
    assert_eq!(
        servers["portable"]["headers"]["Authorization"],
        "Bearer ${PORTABLE_TOKEN}"
    );
    assert_eq!(value["theme"], "dark");
}

#[test]
#[cfg(any(unix, windows))]
fn codex_and_claude_exports_share_portable_http_bindings() {
    let dir = tempfile::tempdir().unwrap();
    let codex = dir.path().join("config.toml");
    let claude = dir.path().join("claude.json");
    std::fs::write(&codex, "[mcp_servers.api]\nurl='https://example.com/mcp'\nbearer_token_env_var='SERVICE_TOKEN'\n[mcp_servers.api.env_http_headers]\nX-Key='SERVICE_KEY'\n").unwrap();
    std::fs::write(&claude, r#"{"mcpServers":{},"theme":"dark"}"#).unwrap();
    let export = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args(["--non-interactive", "codex", "export", "--config"])
        .arg(&codex)
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(export.status.success(), "{export:?}");
    let portable: serde_json::Value = yaml_serde::from_slice(&export.stdout).unwrap();
    let transport = &portable["servers"]["api"]["transport"];
    assert!(portable["servers"]["api"].get("config").is_none());
    assert!(transport.get("bearer_token").is_none());
    assert_eq!(
        transport["headers"]["Authorization"],
        serde_json::json!({"env":"SERVICE_TOKEN","prefix":"Bearer "})
    );
    assert_eq!(transport["headers"]["X-Key"]["env"], "SERVICE_KEY");
    let stack = dir.path().join("stack.yml");
    std::fs::write(&stack, export.stdout).unwrap();
    let applied = run("use", Some(&stack), &claude, &["-y"]);
    assert!(applied.status.success(), "{applied:?}");
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&claude).unwrap()).unwrap();
    assert_eq!(
        value["mcpServers"]["api"]["headers"]["Authorization"],
        "Bearer ${SERVICE_TOKEN}"
    );
    assert_eq!(
        value["mcpServers"]["api"]["headers"]["X-Key"],
        "${SERVICE_KEY}"
    );
    let reexport = run("export", None, &claude, &[]);
    assert!(reexport.status.success(), "{reexport:?}");
    let roundtrip: serde_json::Value = yaml_serde::from_slice(&reexport.stdout).unwrap();
    assert_eq!(roundtrip["servers"]["api"]["transport"], *transport);
}

#[test]
fn codex_export_includes_approval_only_when_requested() {
    let dir = tempfile::tempdir().unwrap();
    let codex = dir.path().join("config.toml");
    std::fs::write(&codex, "[mcp_servers.obsidian]\nurl='https://example.com/mcp'\ndefault_tools_approval_mode='prompt'\n").unwrap();
    let exported = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args(["--non-interactive", "codex", "export", "--config"])
        .arg(&codex)
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(exported.status.success(), "{exported:?}");
    let stack: serde_json::Value = yaml_serde::from_slice(&exported.stdout).unwrap();
    assert_eq!(stack["servers"]["obsidian"]["transport"]["type"], "http");
    assert!(stack["servers"]["obsidian"].get("config").is_none());
    assert!(stack["servers"]["obsidian"].get("permissions").is_none());
    for flag in ["--include-permissions", "--all"] {
        let exported = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
            .args(["--non-interactive", "codex", "export", "--config"])
            .arg(&codex)
            .arg(flag)
            .env_remove("RUST_LOG")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(exported.status.success(), "{exported:?}");
        let stack: serde_json::Value = yaml_serde::from_slice(&exported.stdout).unwrap();
        assert_eq!(
            stack["servers"]["obsidian"]["permissions"]["source"],
            "codex"
        );
        assert_eq!(
            stack["servers"]["obsidian"]["permissions"]["default_mode"],
            "prompt"
        );
    }
}

#[test]
#[cfg(any(unix, windows))]
fn claude_permission_rules_are_opt_in_and_restore_from_settings() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.json");
    let source_settings = dir.path().join("source-settings.json");
    let target = dir.path().join("target.json");
    let target_settings = dir.path().join("target-settings.json");
    let stack = dir.path().join("stack.yml");
    std::fs::write(&source, r#"{"mcpServers":{"obsidian":{"command":"tool"}}}"#).unwrap();
    std::fs::write(&source_settings, r#"{"theme":"dark","permissions":{"allow":["mcp__obsidian__read","Bash"],"ask":["mcp__obsidian__write"],"deny":["mcp__obsidian__delete"]}}"#).unwrap();
    std::fs::write(&target, r#"{"mcpServers":{}}"#).unwrap();
    std::fs::write(
        &target_settings,
        r#"{"permissions":{"allow":["Bash","mcp__obsidian__old"]}}"#,
    )
    .unwrap();
    let exported = run(
        "export",
        None,
        &source,
        &[
            "--include-permissions",
            "--permissions-config",
            source_settings.to_str().unwrap(),
        ],
    );
    assert!(exported.status.success(), "{exported:?}");
    let value: serde_json::Value = yaml_serde::from_slice(&exported.stdout).unwrap();
    assert_eq!(
        value["servers"]["obsidian"]["permissions"]["source"],
        "claude"
    );
    assert_eq!(
        value["servers"]["obsidian"]["permissions"]["allow"],
        serde_json::json!(["mcp__obsidian__read"])
    );
    std::fs::write(&stack, &exported.stdout).unwrap();
    let unchanged = run("use", Some(&stack), &target, &["-y"]);
    assert!(unchanged.status.success(), "{unchanged:?}");
    let settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&target_settings).unwrap()).unwrap();
    assert_eq!(
        settings["permissions"]["allow"],
        serde_json::json!(["Bash", "mcp__obsidian__old"])
    );
    let applied = run(
        "use",
        Some(&stack),
        &target,
        &[
            "-y",
            "--include-permissions",
            "--permissions-config",
            target_settings.to_str().unwrap(),
        ],
    );
    assert!(applied.status.success(), "{applied:?}");
    let settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&target_settings).unwrap()).unwrap();
    assert_eq!(
        settings["permissions"]["allow"],
        serde_json::json!(["Bash", "mcp__obsidian__read"])
    );
    assert_eq!(
        settings["permissions"]["ask"],
        serde_json::json!(["mcp__obsidian__write"])
    );
    assert_eq!(
        settings["permissions"]["deny"],
        serde_json::json!(["mcp__obsidian__delete"])
    );
    assert!(dir.path().join("target-settings.json.~1~").exists());
}

#[test]
#[cfg(any(unix, windows))]
fn untranslatable_codex_mode_warns_and_keeps_claude_rules() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let settings = dir.path().join("settings.json");
    let stack = dir.path().join("stack.yml");
    std::fs::write(&config, r#"{"mcpServers":{}}"#).unwrap();
    let original = r#"{"permissions":{"ask":["mcp__example__*"]}}"#;
    std::fs::write(&settings, original).unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers:\n  example:\n    transport: {type: stdio, command: tool}\n    permissions: {source: codex, default_mode: writes}\n").unwrap();
    let output = run(
        "use",
        Some(&stack),
        &config,
        &[
            "-y",
            "--include-permissions",
            "--permissions-config",
            settings.to_str().unwrap(),
        ],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot reproduce auto or writes"));
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), original);
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
    assert_eq!(value["mcpServers"]["example"]["command"], "tool");
}

#[test]
fn untranslatable_tool_mode_does_not_block_other_permission_rules() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let settings = dir.path().join("settings.json");
    let stack = dir.path().join("stack.yml");
    std::fs::write(&config, r#"{"mcpServers":{}}"#).unwrap();
    std::fs::write(
        &settings,
        r#"{"permissions":{"ask":["mcp__example__write"]}}"#,
    )
    .unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers:\n  example:\n    transport: {type: stdio, command: tool}\n    permissions:\n      source: codex\n      default_mode: auto\n      tools: {read: approve, write: writes}\n").unwrap();
    let result = run(
        "use",
        Some(&stack),
        &config,
        &[
            "-y",
            "--include-permissions",
            "--permissions-config",
            settings.to_str().unwrap(),
        ],
    );
    assert!(result.status.success(), "{result:?}");
    let warnings = String::from_utf8_lossy(&result.stderr);
    assert!(warnings.contains("write"), "{warnings}");
    assert!(warnings.contains("default approval mode"), "{warnings}");
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
    assert_eq!(
        value["permissions"]["allow"],
        serde_json::json!(["mcp__example__read"])
    );
    assert_eq!(
        value["permissions"]["ask"],
        serde_json::json!(["mcp__example__write"])
    );
    let servers: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
    assert_eq!(servers["mcpServers"]["example"]["command"], "tool");
}

#[test]
fn empty_claude_permission_policy_clears_old_rules_when_included() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.json");
    let source_settings = dir.path().join("source-settings.json");
    let target = dir.path().join("target.json");
    let target_settings = dir.path().join("target-settings.json");
    let stack = dir.path().join("stack.yml");
    std::fs::write(&source, r#"{"mcpServers":{"example":{"command":"tool"}}}"#).unwrap();
    std::fs::write(&source_settings, r#"{"permissions":{"allow":["Bash"]}}"#).unwrap();
    std::fs::write(&target, r#"{"mcpServers":{}}"#).unwrap();
    std::fs::write(
        &target_settings,
        r#"{"permissions":{"allow":["Bash","mcp__example__*"]}}"#,
    )
    .unwrap();
    let exported = run(
        "export",
        None,
        &source,
        &[
            "--include-permissions",
            "--permissions-config",
            source_settings.to_str().unwrap(),
        ],
    );
    assert!(exported.status.success(), "{exported:?}");
    let value: serde_json::Value = yaml_serde::from_slice(&exported.stdout).unwrap();
    assert_eq!(
        value["servers"]["example"]["permissions"]["source"],
        "claude"
    );
    std::fs::write(&stack, exported.stdout).unwrap();
    let result = run(
        "use",
        Some(&stack),
        &target,
        &[
            "-y",
            "--include-permissions",
            "--permissions-config",
            target_settings.to_str().unwrap(),
        ],
    );
    assert!(result.status.success(), "{result:?}");
    let settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&target_settings).unwrap()).unwrap();
    assert_eq!(
        settings["permissions"]["allow"],
        serde_json::json!(["Bash"])
    );
}

#[test]
fn conflicting_authorization_headers_fail_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    let original = r#"{"mcpServers":{}}"#;
    std::fs::write(&config, original).unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers:\n  remote:\n    config: {url: 'https://example.com/mcp', http_headers: {authorization: fixture-value}, bearer_token_env_var: SERVICE_TOKEN}\n").unwrap();
    let result = run("use", Some(&stack), &config, &["-y"]);
    assert!(!result.status.success());
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    assert!(!dir.path().join("claude.json.~1~").exists());
}
