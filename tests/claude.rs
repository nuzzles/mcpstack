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
        .env_remove("NO_COLOR")
        .env_remove("MCPSTACK_COLOR")
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
    let plan = String::from_utf8_lossy(&preview.stdout);
    assert!(plan.contains("  + Added: \"api\", \"process\"\n"), "{plan}");
    assert!(plan.contains("  ~ Modified: none\n"), "{plan}");
    assert!(plan.contains("  - Deleted: \"old\"\n"), "{plan}");
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
    let export = run("show", None, &config, &[]);
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
fn claude_shows_changes_before_requiring_approval() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    let original = r#"{"mcpServers":{"old":{"command":"old-tool"}}}"#;
    std::fs::write(&config, original).unwrap();
    std::fs::write(
        &stack,
        "schema_version: 1\nservers:\n  new:\n    config: {command: new-tool}\n",
    )
    .unwrap();
    for operation in ["use", "import"] {
        let output = run(operation, Some(&stack), &config, &[]);
        assert!(!output.status.success(), "{output:?}");
        let plan = String::from_utf8(output.stdout).unwrap();
        assert!(plan.contains("  + Added: \"new\"\n"), "{plan}");
        assert!(
            plan.contains("  - Deleted: \"old\"\n") == (operation == "use"),
            "{plan}"
        );
        assert!(
            !plan.contains(&format!("--- {}", config.display())),
            "{plan}"
        );
        let last = if operation == "use" {
            "- Deleted: \"old\""
        } else {
            "- Deleted: none"
        };
        assert!(plan.trim_end().ends_with(last), "{plan}");
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
        assert!(!dir.path().join("claude.json.~1~").exists());
    }
}

#[test]
fn claude_shows_preliminary_plan_before_a_missing_secret_is_requested() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    let original = r#"{"mcpServers":{}}"#;
    std::fs::write(&config, original).unwrap();
    std::fs::write(
        &stack,
        "schema_version: 1\nservers:\n  new:\n    config:\n      command: new-tool\n      env: {TOKEN: {'$env': MCPSTACK_CLAUDE_TEST_MISSING_PREVIEW_SECRET}}\n",
    )
    .unwrap();
    let output = run("use", Some(&stack), &config, &["-y"]);
    assert!(!output.status.success(), "{output:?}");
    let preview = String::from_utf8(output.stdout).unwrap();
    assert!(preview.contains("Preliminary changes;"), "{preview}");
    assert!(preview.contains("  + Added: \"new\"\n"), "{preview}");
    assert!(
        !preview.contains("MCPSTACK_CLAUDE_TEST_MISSING_PREVIEW_SECRET"),
        "{preview}"
    );
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    assert!(!dir.path().join("claude.json.~1~").exists());
}

#[test]
fn claude_export_then_use_omits_empty_optional_fields_without_a_diff() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    let original = r#"{"theme":"dark","mcpServers":{"linear-server":{"type":"http","url":"https://example.com/mcp"},"plain":{"command":"plain-tool"},"explicit":{"type":"stdio","command":"explicit-tool","args":[],"env":{}},"empty-headers":{"type":"http","url":"https://example.com/other","headers":{}}}}"#;
    std::fs::write(&config, original).unwrap();
    let exported = run("show", None, &config, &[]);
    assert!(exported.status.success(), "{exported:?}");
    std::fs::write(&stack, exported.stdout).unwrap();
    let preview = run("use", Some(&stack), &config, &["--dry-run"]);
    assert!(preview.status.success(), "{preview:?}");
    assert!(
        String::from_utf8_lossy(&preview.stdout).contains("No changes"),
        "{preview:?}"
    );
    assert!(
        !String::from_utf8_lossy(&preview.stdout).contains("@@"),
        "{preview:?}"
    );
    let diff = run("diff", Some(&stack), &config, &[]);
    assert!(diff.status.success(), "{diff:?}");
    assert!(
        !String::from_utf8_lossy(&diff.stdout).contains("@@"),
        "{diff:?}"
    );
    let applied = run("use", Some(&stack), &config, &["-y"]);
    assert!(applied.status.success(), "{applied:?}");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    assert!(!dir.path().join("claude.json.~1~").exists());
}

#[test]
#[cfg(any(unix, windows))]
fn masked_export_and_invalid_import_do_not_expose_or_write_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    std::fs::write(&config,r#"{"mcpServers":{"api":{"type":"http","url":"https://example.com/mcp","headers":{"Authorization":"Bearer fixture-secret"}}}}"#).unwrap();
    let export = run("show", None, &config, &[]);
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
    let export = run("show", None, &config, &[]);
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
        assert!(preview.contains("  ~ Modified: \"api\"\n"), "{preview}");
        assert_eq!(
            preview.contains("<redacted: changed>"),
            operation == "diff",
            "{preview}"
        );
        assert_eq!(
            preview.contains(&format!("--- {}", config.display())),
            operation == "diff",
            "{preview}"
        );
        assert!(!preview.contains("\"$env\""), "{preview}");
        assert!(!preview.contains("old-fixture"));
        assert!(!preview.contains("new-fixture"));
    }
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    assert!(!dir.path().join("claude.json.~1~").exists());
}

#[test]
fn preview_shows_masked_literal_instead_of_synthetic_env_marker() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    let original = r#"{"mcpServers":{"github":{"type":"http","url":"https://example.com/mcp","headers":{"Authorization":"Bearer fixture-secret"}}}}"#;
    std::fs::write(&config, original).unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers:\n  github:\n    transport:\n      type: http\n      url: https://example.com/mcp\n      headers: {Authorization: {env: GITHUB_TOKEN, prefix: 'Bearer '}}\n").unwrap();
    let output = run("diff", Some(&stack), &config, &[]);
    assert!(output.status.success(), "{output:?}");
    let preview = String::from_utf8(output.stdout).unwrap();
    assert!(preview.contains(&format!("--- {}", config.display())));
    assert!(preview.contains("<redacted>"), "{preview}");
    assert!(preview.contains("Bearer ${GITHUB_TOKEN}"), "{preview}");
    assert!(!preview.contains("$env"), "{preview}");
    assert!(!preview.contains("fixture-secret"), "{preview}");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
}

#[test]
fn claude_preview_hunks_use_physical_config_file_lines() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    let existing = serde_json::json!({
        "aaa": ["one", "two", "three", "four"],
        "mcpServers": {"service": {"command": "old-tool"}}
    });
    let original = format!("{}\n", serde_json::to_string_pretty(&existing).unwrap());
    std::fs::write(&config, &original).unwrap();
    std::fs::write(
        &stack,
        "schema_version: 1\nservers:\n  service:\n    config: {command: new-tool}\n",
    )
    .unwrap();
    let line = original
        .lines()
        .position(|line| line.contains("old-tool"))
        .unwrap()
        + 1;
    assert!(line > 5);
    let output = run("diff", Some(&stack), &config, &[]);
    assert!(output.status.success(), "{output:?}");
    let preview = String::from_utf8(output.stdout).unwrap();
    assert!(
        preview.contains(&format!("@@ -{line} +{line} @@")),
        "{preview}"
    );
    assert!(preview.contains("  ~ Modified: \"service\"\n"), "{preview}");
    assert!(
        preview.contains(&format!("--- {}", config.display())),
        "{preview}"
    );
    assert!(
        preview.contains("-      \"command\": \"old-tool\""),
        "{preview}"
    );
    assert!(
        preview.contains("+      \"command\": \"new-tool\""),
        "{preview}"
    );
    assert!(preview.contains("Redacted preview;"), "{preview}");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    let applied = run("use", Some(&stack), &config, &["-y"]);
    assert!(applied.status.success(), "{applied:?}");
    let written = std::fs::read_to_string(&config).unwrap();
    assert!(written.lines().nth(line - 1).unwrap().contains("new-tool"));
}

#[test]
fn whole_file_preview_redacts_unrelated_values_in_minified_json() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    let original = r#"{"local":{"token":"fixture\u002dsecret","pin":123456},"mcpServers":{"service":{"command":"old-tool"}}}"#;
    std::fs::write(&config, original).unwrap();
    std::fs::write(
        &stack,
        "schema_version: 1\nservers:\n  service:\n    config: {command: new-tool}\n",
    )
    .unwrap();
    let output = run("diff", Some(&stack), &config, &[]);
    assert!(output.status.success(), "{output:?}");
    let preview = String::from_utf8(output.stdout).unwrap();
    assert!(preview.contains("@@ -1 +1,"), "{preview}");
    assert!(!preview.contains("fixture-secret"), "{preview}");
    assert!(!preview.contains("fixture\\u002dsecret"), "{preview}");
    assert!(!preview.contains("123456"), "{preview}");
    assert!(preview.contains("<unrelated setting omitted>"), "{preview}");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
}

#[test]
fn claude_preview_and_write_leave_unrelated_json_text_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    let original = concat!(
        "{\n",
        "  \"theme\": \"dark\",\n",
        "  \"projects\": {\"private-path\": {\"mcpServers\": {\"nested\": {\"command\": \"private-tool\"}}}},\n",
        "  \"mcpServers\": {\n",
        "    \"service\": {\n",
        "      \"command\": \"old-tool\"\n",
        "    }\n",
        "  },\n",
        "  \"accessKey\": \"fixture-secret\"\n",
        "}\n"
    );
    std::fs::write(&config, original).unwrap();
    std::fs::write(
        &stack,
        "schema_version: 1\nservers:\n  service:\n    config: {command: new-tool}\n",
    )
    .unwrap();
    let output = run("diff", Some(&stack), &config, &[]);
    assert!(output.status.success(), "{output:?}");
    let preview = String::from_utf8(output.stdout).unwrap();
    assert!(preview.contains("@@ -6 +6 @@"), "{preview}");
    assert!(
        preview.contains("-      \"command\": \"old-tool\""),
        "{preview}"
    );
    assert!(
        preview.contains("+      \"command\": \"new-tool\""),
        "{preview}"
    );
    assert!(!preview.contains("private-path"), "{preview}");
    assert!(!preview.contains("private-tool"), "{preview}");
    assert!(!preview.contains("fixture-secret"), "{preview}");
    assert!(
        !preview.contains("<unrelated setting omitted>"),
        "{preview}"
    );
    let applied = run("use", Some(&stack), &config, &["-y"]);
    assert!(applied.status.success(), "{applied:?}");
    let written = std::fs::read_to_string(&config).unwrap();
    assert_eq!(written, original.replace("old-tool", "new-tool"));
}

#[test]
fn claude_adds_user_scope_servers_without_rewriting_other_fields() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    let original = "{\n  \"theme\": \"dark\",\n  \"projects\": {\"private-path\": true}\n}\n";
    std::fs::write(&config, original).unwrap();
    std::fs::write(
        &stack,
        "schema_version: 1\nservers:\n  service:\n    config: {command: new-tool}\n",
    )
    .unwrap();
    let preview = run("use", Some(&stack), &config, &["--dry-run"]);
    assert!(preview.status.success(), "{preview:?}");
    let text = String::from_utf8(preview.stdout).unwrap();
    assert!(!text.contains("private-path"), "{text}");
    let applied = run("use", Some(&stack), &config, &["-y"]);
    assert!(applied.status.success(), "{applied:?}");
    let written = std::fs::read_to_string(&config).unwrap();
    assert!(written.starts_with(
        "{\n  \"theme\": \"dark\",\n  \"projects\": {\"private-path\": true},\"mcpServers\":"
    ));
    let parsed: serde_json::Value = serde_json::from_str(&written).unwrap();
    assert_eq!(parsed["mcpServers"]["service"]["command"], "new-tool");
}

#[test]
fn claude_previews_follow_shared_ansi_color_modes() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let stack = dir.path().join("stack.yml");
    std::fs::write(
        &config,
        r#"{"mcpServers":{"old":{"command":"old-tool"},"removed":{"command":"removed-tool"}}}"#,
    )
    .unwrap();
    std::fs::write(
        &stack,
        "schema_version: 1\nservers:\n  new:\n    config: {command: new-tool}\n  old:\n    config: {command: changed-tool}\n",
    )
    .unwrap();
    for operation in ["use", "import", "diff"] {
        let execute = |flags: &[&str], no_color: Option<&str>| {
            let mut command = Command::new(env!("CARGO_BIN_EXE_mcpstack"));
            command
                .args(["claude", operation])
                .arg(&stack)
                .arg("--config")
                .arg(&config)
                .args(flags)
                .env_remove("MCPSTACK_COLOR")
                .env_remove("NO_COLOR")
                .env_remove("RUST_LOG")
                .stdin(Stdio::null());
            if operation != "diff" {
                command.arg("--dry-run");
            }
            if operation == "import" {
                command.arg("-y");
            }
            if let Some(value) = no_color {
                command.env("NO_COLOR", value);
            }
            command.output().unwrap()
        };
        let plain = execute(&[], None);
        assert!(plain.status.success(), "{plain:?}");
        assert!(!plain.stdout.contains(&0x1b));
        let colored = execute(&["--color", "always"], None);
        assert!(colored.status.success(), "{colored:?}");
        let text = String::from_utf8(colored.stdout).unwrap();
        assert_eq!(
            text.contains(&format!("\x1b[36m--- {}\x1b[0m", config.display())),
            operation == "diff",
            "{text}"
        );
        assert!(text.contains("\x1b[32m  + Added: \"new\"\x1b[0m"), "{text}");
        assert!(
            text.contains("\x1b[33m  ~ Modified: \"old\"\x1b[0m"),
            "{text}"
        );
        if operation == "use" {
            assert!(
                text.contains("\x1b[31m  - Deleted: \"removed\"\x1b[0m"),
                "{text}"
            );
        }
        let stripped = ["\x1b[36m", "\x1b[32m", "\x1b[33m", "\x1b[31m", "\x1b[0m"]
            .iter()
            .fold(text, |text, sequence| text.replace(sequence, ""));
        assert_eq!(stripped.as_bytes(), plain.stdout);
        assert_eq!(execute(&["--color", "never"], None).stdout, plain.stdout);
        assert_eq!(
            execute(&["--color", "always"], Some("1")).stdout,
            plain.stdout
        );
    }
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
    let result = run("show", None, &config, &[]);
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
    ];
    let mut servers: serde_json::Map<String, serde_json::Value> = names
        .iter()
        .map(|(name, _)| ((*name).into(), serde_json::json!({"command":"tool"})))
        .collect();
    servers.insert("node_repl".into(), serde_json::json!({"command":"tool"}));
    std::fs::write(
        &config,
        serde_json::json!({"mcpServers":servers}).to_string(),
    )
    .unwrap();
    let default = run("show", None, &config, &[]);
    assert!(default.status.success(), "{default:?}");
    let stack: serde_json::Value = yaml_serde::from_slice(&default.stdout).unwrap();
    assert!(stack["servers"].get("node_repl").is_some());
    assert_eq!(stack["servers"].as_object().unwrap().len(), 1);
    for (name, flag) in names {
        let result = run("show", None, &config, &[flag]);
        assert!(result.status.success(), "{result:?}");
        let stack: serde_json::Value = yaml_serde::from_slice(&result.stdout).unwrap();
        let exported = stack["servers"].as_object().unwrap();
        assert_eq!(exported.len(), 2);
        assert!(exported.contains_key(name));
        assert!(exported.contains_key("node_repl"));
    }
    let all = run("show", None, &config, &["--all"]);
    assert!(all.status.success(), "{all:?}");
    let stack: serde_json::Value = yaml_serde::from_slice(&all.stdout).unwrap();
    assert_eq!(stack["servers"].as_object().unwrap().len(), names.len() + 1);
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
        .args(["--non-interactive", "codex", "show", "--config"])
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
    let reexport = run("show", None, &claude, &[]);
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
        .args(["--non-interactive", "codex", "show", "--config"])
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
            .args(["--non-interactive", "codex", "show", "--config"])
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
        "show",
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
fn permission_diff_shows_rule_moves_and_matches_written_settings() {
    for pretty in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("claude.json");
        let settings = dir.path().join("custom-settings.json");
        let stack = dir.path().join("stack.yml");
        let original_config = r#"{"mcpServers":{"example":{"command":"tool"}}}"#;
        std::fs::write(&config, original_config).unwrap();
        let value = serde_json::json!({
            "env": {"TOKEN": "fixture-secret", "PIN": 123456},
            "permissions": {
                "allow": ["Bash(curl --token fixture-secret)", "mcp__example__old"],
                "ask": ["mcp__example__read"],
                "deny": ["mcp__example__delete"],
                "additionalDirectories": ["private-path", "another-private-path"]
            }
        });
        let original = if pretty {
            format!("{}\n", serde_json::to_string_pretty(&value).unwrap())
        } else {
            serde_json::to_string(&value)
                .unwrap()
                .replace("fixture-secret", "fixture\\u002dsecret")
        };
        std::fs::write(&settings, &original).unwrap();
        std::fs::write(&stack, "schema_version: 1\nservers:\n  example:\n    transport: {type: stdio, command: tool}\n    permissions:\n      source: claude\n      allow: [mcp__example__read]\n      ask: [mcp__example__write]\n      deny: []\n").unwrap();
        let flags = [
            "--include-permissions",
            "--permissions-config",
            settings.to_str().unwrap(),
            "--color",
            "never",
        ];
        let output = run("diff", Some(&stack), &config, &flags);
        assert!(output.status.success(), "{output:?}");
        let preview = String::from_utf8(output.stdout.clone()).unwrap();
        assert!(
            preview.contains(&format!(
                "--- {}\n+++ {}\n",
                settings.display(),
                settings.display()
            )),
            "{preview}"
        );
        assert!(
            !preview.contains(&format!("--- {}", config.display())),
            "{preview}"
        );
        for rule in [
            "mcp__example__old",
            "mcp__example__read",
            "mcp__example__delete",
        ] {
            assert!(
                preview
                    .lines()
                    .any(|line| line.starts_with('-') && line.contains(rule)),
                "{preview}"
            );
        }
        for rule in ["mcp__example__read", "mcp__example__write"] {
            assert!(
                preview
                    .lines()
                    .any(|line| line.starts_with('+') && line.contains(rule)),
                "{preview}"
            );
        }
        for bucket in ["allow", "ask", "deny"] {
            assert!(preview.contains(&format!("\"{bucket}\"")), "{preview}");
        }
        for secret in [
            "fixture-secret",
            "fixture\\u002dsecret",
            "123456",
            "private-path",
            "Bash(curl",
        ] {
            assert!(!preview.contains(secret), "{preview}");
            assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
        }
        assert!(!preview.contains('\x1b'));
        assert_eq!(
            run("diff", Some(&stack), &config, &flags).stdout,
            output.stdout
        );
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), original);
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original_config);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);

        let mut apply_flags = flags.to_vec();
        apply_flags.push("-y");
        let applied = run("import", Some(&stack), &config, &apply_flags);
        assert!(applied.status.success(), "{applied:?}");
        let written = std::fs::read_to_string(&settings).unwrap();
        let actual: serde_json::Value = serde_json::from_str(&written).unwrap();
        assert_eq!(actual["env"], value["env"]);
        assert_eq!(
            actual["permissions"]["additionalDirectories"],
            value["permissions"]["additionalDirectories"]
        );
        assert_eq!(
            actual["permissions"]["allow"],
            serde_json::json!(["Bash(curl --token fixture-secret)", "mcp__example__read"])
        );
        assert_eq!(
            actual["permissions"]["ask"],
            serde_json::json!(["mcp__example__write"])
        );
        assert_eq!(actual["permissions"]["deny"], serde_json::json!([]));
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original_config);
        assert!(!dir.path().join("claude.json.~1~").exists());
        assert_permission_diff_coordinates(&preview, &original, &written);
        let repeated = run("diff", Some(&stack), &config, &flags);
        assert!(repeated.status.success(), "{repeated:?}");
        assert!(repeated.stdout.is_empty(), "{repeated:?}");
    }
}

// Check that visible rules in each hunk point to the actual source/write lines,
// even when redaction hides multiline settings or the source is minified.
fn assert_permission_diff_coordinates(preview: &str, original: &str, written: &str) {
    let before: Vec<_> = original.lines().collect();
    let after: Vec<_> = written.lines().collect();
    let (mut old, mut new) = (0, 0);
    for line in preview.lines() {
        if line.starts_with("@@ ") {
            let mut ranges = line.split_whitespace().skip(1);
            let start = |range: &str| {
                range[1..]
                    .split(',')
                    .next()
                    .unwrap()
                    .parse::<usize>()
                    .unwrap()
            };
            old = start(ranges.next().unwrap());
            new = start(ranges.next().unwrap());
        } else if !line.starts_with("--- ") && !line.starts_with("+++ ") {
            if line.contains("mcp__") {
                // The fixture has no escapes in rule spellings. Minified lines
                // contain omitted values, so compare each visible rule token.
                for rule in line.split('"').filter(|s| s.starts_with("mcp__")) {
                    if line.starts_with(['-', ' ']) {
                        assert!(before[old - 1].contains(rule), "{line}");
                    }
                    if line.starts_with(['+', ' ']) {
                        assert!(after[new - 1].contains(rule), "{line}");
                    }
                }
            }
            if line.starts_with(['-', ' ']) {
                old += 1;
            }
            if line.starts_with(['+', ' ']) {
                new += 1;
            }
        }
    }
}

#[test]
fn permission_diff_handles_missing_settings_and_color_without_writes() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let settings = dir.path().join("missing/settings.json");
    let stack = dir.path().join("stack.yml");
    std::fs::write(&config, r#"{"mcpServers":{"example":{"command":"tool"}}}"#).unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers:\n  example:\n    transport: {type: stdio, command: tool}\n    permissions: {source: claude, allow: [mcp__example__read], ask: [mcp__example__write], deny: [mcp__example__delete]}\n").unwrap();
    let execute = |flags: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_mcpstack"))
            .args(["--non-interactive", "claude", "diff"])
            .arg(&stack)
            .arg("--config")
            .arg(&config)
            .args(["--include-permissions", "--permissions-config"])
            .arg(&settings)
            .args(flags)
            .env_remove("MCPSTACK_COLOR")
            .env_remove("NO_COLOR")
            .env_remove("RUST_LOG")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let plain = execute(&["--color", "never"]);
    assert!(plain.status.success(), "{plain:?}");
    let text = String::from_utf8(plain.stdout.clone()).unwrap();
    assert!(text.contains("--- /dev/null\n"), "{text}");
    assert!(
        text.contains(&format!("+++ {}\n", settings.display())),
        "{text}"
    );
    for bucket in ["allow", "ask", "deny"] {
        assert!(text.contains(&format!("+    \"{bucket}\": [")), "{text}");
    }
    let colored = execute(&["--color", "always"]);
    assert!(colored.status.success(), "{colored:?}");
    let color_text = String::from_utf8(colored.stdout).unwrap();
    assert!(
        color_text.contains("\x1b[36m--- /dev/null\x1b[0m"),
        "{color_text}"
    );
    assert!(color_text.contains("\x1b[32m+"), "{color_text}");
    let stripped = ["\x1b[36m", "\x1b[32m", "\x1b[31m", "\x1b[0m"]
        .iter()
        .fold(color_text, |text, code| text.replace(code, ""));
    assert_eq!(stripped, text);
    assert_eq!(
        execute(&["--color", "always", "--no-color"]).stdout,
        plain.stdout
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    assert!(!settings.parent().unwrap().exists());
}

#[test]
fn permission_diff_respects_opt_in_selection_and_codex_translation() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let settings = dir.path().join("settings.json");
    let stack = dir.path().join("stack.yml");
    std::fs::write(
        &config,
        r#"{"mcpServers":{"example":{"command":"tool"},"other":{"command":"tool"}}}"#,
    )
    .unwrap();
    std::fs::write(&settings, "{}\n").unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers:\n  example:\n    transport: {type: stdio, command: tool}\n    permissions: {source: codex, default_mode: auto, tools: {read: approve, write: prompt}}\n  other:\n    transport: {type: stdio, command: tool}\n    permissions: {source: claude, deny: [mcp__other__*]}\n").unwrap();
    let omitted = run("diff", Some(&stack), &config, &[]);
    assert!(omitted.status.success(), "{omitted:?}");
    assert!(omitted.stdout.is_empty(), "{omitted:?}");
    let selected = run(
        "diff",
        Some(&stack),
        &config,
        &[
            "--include-permissions",
            "--server",
            "example",
            "--color",
            "never",
        ],
    );
    assert!(selected.status.success(), "{selected:?}");
    let text = String::from_utf8(selected.stdout).unwrap();
    assert!(text.contains("mcp__example__read"), "{text}");
    assert!(text.contains("mcp__example__write"), "{text}");
    assert!(!text.contains("mcp__example__*"), "{text}");
    assert!(!text.contains("mcp__other"), "{text}");
    assert!(
        String::from_utf8_lossy(&selected.stderr).contains("Skipping Codex default approval mode")
    );
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), "{}\n");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
}

#[test]
fn permission_diff_rejects_malformed_settings_without_exposing_values() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let settings = dir.path().join("settings.json");
    let stack = dir.path().join("stack.yml");
    std::fs::write(&config, r#"{"mcpServers":{"example":{"command":"tool"}}}"#).unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers:\n  example:\n    transport: {type: stdio, command: tool}\n    permissions: {source: claude, allow: [mcp__example]}\n").unwrap();
    for invalid in [
        r#"{"permissions":{"allow":["fixture-secret",123]}}"#,
        r#"{"permissions":{"allow":"fixture-secret"}}"#,
        "{invalid fixture-secret",
    ] {
        std::fs::write(&settings, invalid).unwrap();
        let output = run("diff", Some(&stack), &config, &["--include-permissions"]);
        assert!(!output.status.success(), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-secret"));
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), invalid);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
    }
}

#[test]
fn permission_summaries_classify_rules_and_precede_approval_without_writes() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let settings = dir.path().join("custom-settings.json");
    let stack = dir.path().join("stack.yml");
    let original_config = r#"{"mcpServers":{"example":{"command":"tool"}}}"#;
    let original_settings = serde_json::json!({
        "env": {"TOKEN": "fixture-secret"},
        "permissions": {
            "allow": ["mcp__example__old", "mcp__example__multi", "Bash(fixture-secret)"],
            "ask": ["mcp__example__read", "mcp__example__multi"],
            "deny": ["mcp__example__delete"]
        }
    })
    .to_string();
    std::fs::write(&config, original_config).unwrap();
    std::fs::write(&settings, &original_settings).unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers:\n  example:\n    transport: {type: stdio, command: tool}\n    permissions:\n      source: claude\n      allow: [mcp__example__read, mcp__example__multi]\n      ask: [mcp__example__write]\n      deny: [mcp__example__multi]\n").unwrap();
    let expected = concat!(
        "MCP permission changes:\n",
        "  + Added: \"mcp__example__write\" (ask)\n",
        "  ~ Modified: \"mcp__example__multi\" (allow, ask -> allow, deny), \"mcp__example__read\" (ask -> allow)\n",
        "  - Deleted: \"mcp__example__delete\" (deny), \"mcp__example__old\" (allow)\n",
    );
    for operation in ["use", "import"] {
        let flags = [
            "--include-permissions",
            "--permissions-config",
            settings.to_str().unwrap(),
        ];
        let mut dry_run = flags.to_vec();
        dry_run.push("--dry-run");
        if operation == "import" {
            dry_run.push("-y");
        }
        let output = run(operation, Some(&stack), &config, &dry_run);
        assert!(output.status.success(), "{output:?}");
        let preview = String::from_utf8(output.stdout.clone()).unwrap();
        assert!(preview.contains(expected), "{preview}");
        assert!(
            preview.contains(&format!(
                "Target settings: {}",
                settings.to_string_lossy().escape_debug()
            )),
            "{preview}"
        );
        assert!(
            !preview.contains("Permission rules would change"),
            "{preview}"
        );
        assert!(!preview.contains("@@"), "{preview}");
        assert!(!preview.contains("fixture-secret"), "{preview}");
        // The full summary is available before the unattended approval error.
        let unapproved = run(operation, Some(&stack), &config, &flags);
        assert!(!unapproved.status.success(), "{unapproved:?}");
        assert!(
            String::from_utf8_lossy(&unapproved.stdout).contains(expected),
            "{unapproved:?}"
        );
        let mut color_flags = dry_run.clone();
        color_flags.extend(["--color", "always"]);
        let colored = run(operation, Some(&stack), &config, &color_flags);
        assert!(colored.status.success(), "{colored:?}");
        let text = String::from_utf8(colored.stdout).unwrap();
        for (code, marker) in [(32, '+'), (33, '~'), (31, '-')] {
            assert!(text.contains(&format!("\x1b[{code}m  {marker}")), "{text}");
        }
        let plain = ["\x1b[32m", "\x1b[33m", "\x1b[31m", "\x1b[0m"]
            .iter()
            .fold(text, |text, code| text.replace(code, ""));
        assert_eq!(plain, preview);
        color_flags.push("--no-color");
        assert_eq!(
            run(operation, Some(&stack), &config, &color_flags).stdout,
            output.stdout
        );
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original_config);
        assert_eq!(
            std::fs::read_to_string(&settings).unwrap(),
            original_settings
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
    }
}

#[test]
fn permission_summary_distinguishes_normalization_from_noop_and_filters_selection() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let settings = dir.path().join("settings.json");
    let stack = dir.path().join("stack.yml");
    std::fs::write(
        &config,
        r#"{"mcpServers":{"example":{"command":"tool"},"other":{"command":"tool"}}}"#,
    )
    .unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers:\n  example:\n    transport: {type: stdio, command: tool}\n    permissions: {source: claude, allow: [mcp__example]}\n  other:\n    transport: {type: stdio, command: tool}\n    permissions: {source: claude, deny: [mcp__other]}\n").unwrap();
    for operation in ["use", "import"] {
        std::fs::write(
            &settings,
            r#"{"permissions":{"allow":["mcp__example","mcp__example"],"deny":["mcp__other"]}}"#,
        )
        .unwrap();
        let flags = ["--include-permissions", "--dry-run", "-y"];
        let normalized = run(operation, Some(&stack), &config, &flags);
        assert!(normalized.status.success(), "{normalized:?}");
        let text = String::from_utf8(normalized.stdout).unwrap();
        assert!(
            text.contains("  + Added: none\n  ~ Modified: none\n  - Deleted: none\n"),
            "{text}"
        );
        assert!(
            text.contains("rule order or duplicate entries would be normalized"),
            "{text}"
        );
        std::fs::write(
            &settings,
            r#"{"permissions":{"allow":["mcp__example"],"deny":["mcp__other"]}}"#,
        )
        .unwrap();
        let noop = run(operation, Some(&stack), &config, &flags);
        assert!(noop.status.success(), "{noop:?}");
        let text = String::from_utf8(noop.stdout).unwrap();
        assert!(text.contains("No changes;"), "{text}");
        assert!(!text.contains("MCP permission changes:"), "{text}");
    }
    std::fs::write(&settings, "{}").unwrap();
    let selected = run(
        "import",
        Some(&stack),
        &config,
        &[
            "--include-permissions",
            "--dry-run",
            "-y",
            "--server",
            "example",
        ],
    );
    assert!(selected.status.success(), "{selected:?}");
    let text = String::from_utf8(selected.stdout).unwrap();
    assert!(
        text.contains("  + Added: \"mcp__example\" (allow)\n"),
        "{text}"
    );
    assert!(!text.contains("mcp__other"), "{text}");
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), "{}");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
}

#[test]
#[cfg(any(unix, windows))]
fn claude_prepares_both_files_before_writes_and_repeats_without_backups() {
    for operation in ["import", "use"] {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("claude.json");
        let settings = dir.path().join("settings.json");
        let stack = dir.path().join("stack.yml");
        let original = "{\"theme\":\"dark\",\"mcpServers\":{}}\n";
        let original_settings = "{\"permissions\":{\"allow\":[\"Bash\"]}}\n";
        std::fs::write(&config, original).unwrap();
        std::fs::write(&stack, "schema_version: 1\nservers:\n  example:\n    transport: {type: stdio, command: tool}\n    permissions: {source: claude, allow: [mcp__example]}\n").unwrap();
        let flags = &["-y", "--include-permissions"];
        // Malformed second file fails before creating backups or changing servers.
        std::fs::write(&settings, "{invalid fixture-secret").unwrap();
        let failed = run(operation, Some(&stack), &config, flags);
        assert!(!failed.status.success());
        assert!(!String::from_utf8_lossy(&failed.stderr).contains("fixture-secret"));
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
        std::fs::write(&settings, original_settings).unwrap();
        let preview = run(
            operation,
            Some(&stack),
            &config,
            &["-y", "--include-permissions", "--dry-run"],
        );
        assert!(preview.status.success(), "{preview:?}");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
        assert_eq!(
            std::fs::read_to_string(&settings).unwrap(),
            original_settings
        );
        let applied = run(operation, Some(&stack), &config, flags);
        assert!(applied.status.success(), "{applied:?}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("claude.json.~1~")).unwrap(),
            original
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("settings.json.~1~")).unwrap(),
            original_settings
        );
        let written = std::fs::read(&config).unwrap();
        let written_settings = std::fs::read(&settings).unwrap();
        let servers: serde_json::Value = serde_json::from_slice(&written).unwrap();
        let permissions: serde_json::Value = serde_json::from_slice(&written_settings).unwrap();
        assert_eq!(servers["mcpServers"]["example"]["command"], "tool");
        assert_eq!(servers["theme"], "dark");
        assert_eq!(
            permissions["permissions"]["allow"],
            serde_json::json!(["Bash", "mcp__example"])
        );
        let repeated = run(operation, Some(&stack), &config, flags);
        assert!(repeated.status.success(), "{repeated:?}");
        assert_eq!(std::fs::read(&config).unwrap(), written);
        assert_eq!(std::fs::read(&settings).unwrap(), written_settings);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 5);
    }
}

#[test]
#[cfg(any(unix, windows))]
fn claude_server_changes_do_not_require_an_unchanged_settings_directory() {
    for operation in ["import", "use"] {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("claude.json");
        let settings = dir.path().join("missing/nested/settings.json");
        let stack = dir.path().join("stack.yml");
        let original = "{\"theme\":\"dark\",\"mcpServers\":{}}\n";
        std::fs::write(&config, original).unwrap();
        std::fs::write(
            &stack,
            "schema_version: 1\nservers:\n  example:\n    transport: {type: stdio, command: tool}\n",
        )
        .unwrap();
        let flags = [
            "-y",
            "--include-permissions",
            "--permissions-config",
            settings.to_str().unwrap(),
        ];
        let mut preview_flags = flags.to_vec();
        preview_flags.push("--dry-run");
        let preview = run(operation, Some(&stack), &config, &preview_flags);
        assert!(preview.status.success(), "{preview:?}");
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);

        let applied = run(operation, Some(&stack), &config, &flags);
        assert!(applied.status.success(), "{applied:?}");
        let written = std::fs::read(&config).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&written).unwrap();
        assert_eq!(value["mcpServers"]["example"]["command"], "tool");
        assert_eq!(value["theme"], "dark");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("claude.json.~1~")).unwrap(),
            original
        );
        let repeated = run(operation, Some(&stack), &config, &flags);
        assert!(repeated.status.success(), "{repeated:?}");
        assert_eq!(std::fs::read(&config).unwrap(), written);
        assert!(!dir.path().join("missing").exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
    }
}

#[test]
#[cfg(any(unix, windows))]
fn claude_rejects_aliasing_targets_and_backup_failure_before_config_writes() {
    for operation in ["import", "use"] {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("claude.json");
        let settings = dir.path().join("settings.json");
        let stack = dir.path().join("stack.yml");
        let original = "{}";
        std::fs::write(&config, original).unwrap();
        std::fs::write(&settings, original).unwrap();
        std::fs::write(&stack, "schema_version: 1\nservers:\n  example:\n    transport: {type: stdio, command: tool}\n    permissions: {source: claude, allow: [mcp__example]}\n").unwrap();
        let failed = run(
            operation,
            Some(&stack),
            &config,
            &[
                "-y",
                "--include-permissions",
                "--permissions-config",
                config.to_str().unwrap(),
            ],
        );
        assert!(!failed.status.success());
        assert!(String::from_utf8_lossy(&failed.stderr).contains("different files"));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
        std::fs::write(
            dir.path().join(format!("settings.json.~{}~", u64::MAX)),
            "keep",
        )
        .unwrap();
        let failed = run(
            operation,
            Some(&stack),
            &config,
            &["-y", "--include-permissions"],
        );
        assert!(!failed.status.success());
        assert!(String::from_utf8_lossy(&failed.stderr).contains("BACKUP_ERROR"));
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), original);
    }
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
        "show",
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
