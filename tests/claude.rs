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
    assert!(yaml.contains("$env"));
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
    std::fs::write(&stack, "schema_version: 1\nservers:\n  computer-use:\n    config: {command: tool, cwd: '/private/fixture-secret', enabled: true}\n  github:\n    config: {url: 'https://example.com/mcp', bearer_token_env_var: PRIVATE_TOKEN}\n  remote:\n    transport: {type: http, url: 'https://example.com/mcp', bearer_token: {env: PRIVATE_TOKEN}}\n").unwrap();
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
#[cfg(any(unix, windows))]
fn codex_http_headers_and_runtime_bindings_map_to_claude_headers() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    std::fs::write(&config, r#"{"mcpServers":{},"theme":"dark"}"#).unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers:\n  direct:\n    config: {url: 'https://example.com/mcp', http_headers: {X-Test: fixture-value}}\n  bearer:\n    config: {url: 'https://example.com/mcp', bearer_token_env_var: SERVICE_TOKEN}\n  environment:\n    config: {url: 'https://example.com/mcp', env_http_headers: {X-API-Key: SERVICE_KEY}}\n  portable:\n    transport: {type: http, url: 'https://example.com/mcp', bearer_token: {env: PORTABLE_TOKEN}}\n").unwrap();
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
