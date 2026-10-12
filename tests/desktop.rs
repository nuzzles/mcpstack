use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

fn command() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mcpstack"));
    cmd.args(["claude-desktop", "--non-interactive", "--no-color"])
        .env_remove("RUST_LOG")
        .env_remove("MCPSTACK_COLOR")
        .env("MCPSTACK_SKIP_VERSION_CHECK", "true")
        .stdin(Stdio::null());
    cmd
}

fn run(operation: &str, stack: Option<&Path>, config: &Path, flags: &[&str]) -> Output {
    let mut cmd = command();
    cmd.arg(operation);
    if let Some(stack) = stack {
        cmd.arg(stack);
    }
    cmd.arg("--config")
        .arg(config)
        .args(flags)
        .output()
        .unwrap()
}

fn success(output: Output) -> String {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

fn config(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn files(path: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    files.sort();
    files
}

#[test]
fn desktop_import_switch_preview_and_repeat_preserve_settings_and_backups() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("desktop.json");
    let stack = dir.path().join("stack.yaml");
    let original = "{\n  \"preferences\" : { \"theme\": \"dark\", \"mcpServers\": { \"nested\": 7 } },\n  \"mcpServers\": {\"old\":{\"command\":\"old-tool\"},\"keep\":{\"command\":\"keep-tool\",\"env\":{}}}\n}\n";
    fs::write(&target, original).unwrap();
    fs::write(
        dir.path().join("settings.json"),
        "unrelated Desktop settings",
    )
    .unwrap();
    fs::write(&stack, "schema_version: 1\nservers:\n  keep:\n    transport: {type: stdio, command: keep-tool}\n  new:\n    transport: {type: stdio, command: new-tool, args: [example]}\n").unwrap();
    let before = files(dir.path());
    for (operation, flags) in [
        ("diff", vec![]),
        ("import", vec!["--dry-run", "-y"]),
        ("use", vec!["--dry-run"]),
    ] {
        let output = success(run(operation, Some(&stack), &target, &flags));
        assert!(output.contains("new"), "{output}");
        if operation == "use" {
            assert!(output.contains("Deleted: \"old\""));
        }
        assert_eq!(fs::read_to_string(&target).unwrap(), original);
        assert_eq!(files(dir.path()), before);
    }
    success(run("import", Some(&stack), &target, &["-y"]));
    let imported = config(&target);
    assert_eq!(imported["mcpServers"].as_object().unwrap().len(), 3);
    assert_eq!(
        imported["mcpServers"]["new"],
        json!({"command":"new-tool","args":["example"]})
    );
    assert!(
        fs::read_to_string(&target).unwrap().contains(
            "\"preferences\" : { \"theme\": \"dark\", \"mcpServers\": { \"nested\": 7 } }"
        )
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("desktop.json.~1~")).unwrap(),
        original
    );
    let after = fs::read(&target).unwrap();
    let entries = files(dir.path());
    assert!(success(run("import", Some(&stack), &target, &["-y"])).contains("No changes"));
    assert_eq!(fs::read(&target).unwrap(), after);
    assert_eq!(files(dir.path()), entries);
    success(run("use", Some(&stack), &target, &["-y"]));
    assert!(config(&target)["mcpServers"].get("old").is_none());
    let after = fs::read(&target).unwrap();
    let entries = files(dir.path());
    assert!(success(run("use", Some(&stack), &target, &["-y"])).contains("No changes"));
    assert_eq!(fs::read(&target).unwrap(), after);
    assert_eq!(files(dir.path()), entries);
    fs::write(&stack, "schema_version: 1\nservers: {}\n").unwrap();
    success(run("use", Some(&stack), &target, &["-y"]));
    assert_eq!(config(&target)["mcpServers"], json!({}));
    assert_eq!(
        fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        "unrelated Desktop settings"
    );
}

#[test]
fn desktop_has_no_code_reserved_names_and_supports_selection() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("desktop.json");
    let original = json!({"mcpServers":{
        "workspace":{"command":"workspace-tool"},
        "computer-use":{"command":"computer-tool"},
        "Claude Browser":{"command":"browser-tool"},
        "node_repl":{"command":"node-tool"}
    }});
    fs::write(&target, original.to_string()).unwrap();
    let listing = success(run("list", None, &target, &[]));
    for name in ["workspace", "computer-use", "Claude Browser", "node_repl"] {
        assert!(listing.contains(name));
    }
    assert!(!listing.contains("ignored"));
    let selected = success(run("list", None, &target, &["--server", "workspace"]));
    assert!(selected.contains("workspace"));
    assert!(!selected.contains("computer-use"));
    let stack = dir.path().join("all.yaml");
    success(run("export", Some(&stack), &target, &[]));
    assert!(success(run("use", Some(&stack), &target, &["-y"])).contains("No changes"));
    assert_eq!(config(&target), original);
    let invalid = run("show", None, &target, &["--server", "unknown"]);
    assert_eq!(invalid.status.code(), Some(2));
    assert!(invalid.stdout.is_empty());
    let partial = dir.path().join("partial.yaml");
    success(run(
        "export",
        Some(&partial),
        &target,
        &["--server", "workspace"],
    ));
    let selected_target = dir.path().join("selected.json");
    success(run(
        "import",
        Some(&stack),
        &selected_target,
        &["--server", "workspace", "-y"],
    ));
    assert_eq!(
        config(&selected_target)["mcpServers"]
            .as_object()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        success(run("show", None, &selected_target, &[])),
        fs::read_to_string(partial).unwrap()
    );
}

#[test]
fn desktop_round_trips_literal_templates_and_masks_sensitive_ones() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("desktop.json");
    let original = json!({"mcpServers":{"example":{
        "command":"example-tool", "args":["${LITERAL_ARG}", "--token", "${LITERAL_TOKEN}"],
        "env":{"NORMAL":"${LITERAL_ENV}", "API_KEY":"${LITERAL_API_KEY}"}
    }}});
    fs::write(&target, original.to_string()).unwrap();
    let masked = success(run("show", None, &target, &[]));
    assert!(masked.contains("${LITERAL_ARG}"));
    assert!(masked.contains("${LITERAL_ENV}"));
    assert!(!masked.contains("${LITERAL_TOKEN}"));
    assert!(!masked.contains("${LITERAL_API_KEY}"));
    assert!(masked.contains("value_env:"));
    let stack = dir.path().join("stack.yaml");
    success(run("export", Some(&stack), &target, &["--expose-secrets"]));
    let exported = fs::read_to_string(&stack).unwrap();
    assert!(exported.contains("${LITERAL_TOKEN}"));
    assert!(!exported.contains("env: LITERAL_"));
    let before = fs::read(&target).unwrap();
    assert!(success(run("use", Some(&stack), &target, &["-y"])).contains("No changes"));
    assert_eq!(fs::read(&target).unwrap(), before);
    fs::write(
        &stack,
        exported
            .replace("${LITERAL_TOKEN}", "new-private-token")
            .replace("${LITERAL_API_KEY}", "new-private-key"),
    )
    .unwrap();
    let diff = success(run("diff", Some(&stack), &target, &[]));
    for secret in [
        "${LITERAL_TOKEN}",
        "${LITERAL_API_KEY}",
        "new-private-token",
        "new-private-key",
    ] {
        assert!(!diff.contains(secret), "{diff}");
    }
    assert!(diff.contains("<redacted: changed>"));
}

#[test]
fn desktop_shares_portable_stdio_stacks_with_code_and_codex() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("desktop.json");
    fs::write(&target, r#"{"mcpServers":{"example":{"command":"example-tool","args":["--mode","read"],"env":{"MODE":"safe"}}}}"#).unwrap();
    let stack = dir.path().join("stack.yaml");
    success(run("export", Some(&stack), &target, &[]));
    for harness in ["codex", "claude"] {
        let native = dir.path().join(format!("{harness}.config"));
        let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
            .args([harness, "use"])
            .arg(&stack)
            .arg("--config")
            .arg(&native)
            .args(["--non-interactive", "-y", "--no-color"])
            .env_remove("RUST_LOG")
            .output()
            .unwrap();
        success(output);
        let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
            .args([harness, "show", "--config"])
            .arg(&native)
            .arg("--non-interactive")
            .env_remove("RUST_LOG")
            .output()
            .unwrap();
        let returned = dir.path().join(format!("{harness}.yaml"));
        fs::write(&returned, success(output)).unwrap();
        assert!(success(run("use", Some(&returned), &target, &["-y"])).contains("No changes"));
    }
}

#[test]
fn desktop_ignores_stack_permissions_without_reading_or_writing_code_settings() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("desktop.json");
    let stack = dir.path().join("stack.yaml");
    fs::write(
        dir.path().join("settings.json"),
        "not JSON; must not be read",
    )
    .unwrap();
    fs::write(&stack, "schema_version: 1\nservers:\n  example:\n    transport: {type: stdio, command: example-tool}\n    permissions: {allow: ['*'], deny: [write]}\n").unwrap();
    success(run("use", Some(&stack), &target, &["-y"]));
    assert_eq!(
        config(&target)["mcpServers"]["example"],
        json!({"command":"example-tool"})
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        "not JSON; must not be read"
    );
    assert!(!success(run("show", None, &target, &[])).contains("permissions:"));
    for operation in ["show", "export", "import", "use", "diff"] {
        for flag in ["--include-permissions", "--permissions-config"] {
            let output = run(
                operation,
                if operation == "show" {
                    None
                } else {
                    Some(&stack)
                },
                &target,
                &[flag],
            );
            assert_eq!(output.status.code(), Some(2), "{operation}: {output:?}");
            assert!(output.stdout.is_empty());
        }
    }
}

#[test]
fn desktop_unsupported_transports_and_settings_fail_before_any_write() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("desktop.json");
    let stack = dir.path().join("stack.yaml");
    let original =
        r#"{"mcpServers":{"old":{"command":"old-tool"}},"preferences":{"theme":"dark"}}"#;
    fs::write(&target, original).unwrap();
    let cases = [
        "transport: {type: http, url: 'https://example.com/mcp'}",
        "transport: {type: sse, url: 'https://example.com/sse'}",
        "transport: {type: websocket, url: 'wss://example.com/ws'}",
        "transport: {type: stdio, command: example, cwd: /tmp}",
        "transport: {type: stdio, command: example, env_vars: [VARIABLE]}",
        "transport: {type: stdio, command: example}\n    settings: {enabled: false}",
        "transport: {type: stdio, command: example}\n    settings: {startup_timeout_sec: 3}",
        "config: {type: http, url: 'https://example.com/private-secret'}",
        "config: {command: example, unknown: private-secret}",
    ];
    for case in cases {
        fs::write(&stack, format!("schema_version: 1\nservers:\n  valid:\n    transport: {{type: stdio, command: valid}}\n  invalid:\n    {case}\n")).unwrap();
        let before = files(dir.path());
        for operation in ["diff", "import", "use"] {
            let flags = if operation == "diff" {
                vec![]
            } else {
                vec!["-y"]
            };
            let output = run(operation, Some(&stack), &target, &flags);
            assert!(!output.status.success(), "{case}: {output:?}");
            assert!(!String::from_utf8_lossy(&output.stderr).contains("private-secret"));
            assert!(String::from_utf8_lossy(&output.stderr).contains("Claude Desktop"));
            assert_eq!(fs::read_to_string(&target).unwrap(), original);
            assert_eq!(files(dir.path()), before);
        }
    }
}

#[test]
fn desktop_failures_leave_config_and_exports_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("desktop.json");
    let stack = dir.path().join("stack.yaml");
    fs::write(&target, r#"{"mcpServers":{},"preferences":{"foo":true}}"#).unwrap();
    fs::write(&stack, "schema_version: 1\nservers:\n  sample:\n    config: {command: example, env: {TOKEN: {'$env': MCPSTACK_DESKTOP_ABSENT_SECRET}}}\n").unwrap();
    let before = fs::read(&target).unwrap();
    let entries = files(dir.path());
    let output = command()
        .arg("use")
        .arg(&stack)
        .arg("--config")
        .arg(&target)
        .arg("-y")
        .env_remove("MCPSTACK_DESKTOP_ABSENT_SECRET")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read(&target).unwrap(), before);
    assert_eq!(files(dir.path()), entries);
    fs::write(
        &stack,
        "schema_version: 1\nservers:\n  sample:\n    config: {command: example}\n",
    )
    .unwrap();
    for op in ["use", "import"] {
        let output = run(op, Some(&stack), &target, &[]);
        assert_eq!(output.status.code(), Some(10));
        assert!(String::from_utf8_lossy(&output.stdout).contains("sample"));
        assert_eq!(fs::read(&target).unwrap(), before);
    }
    let output = run("export", Some(&stack), &target, &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(fs::read_to_string(&stack).unwrap().contains("sample"));
    success(run("export", Some(&stack), &target, &["--overwrite"]));
    for invalid in [
        r#"{"mcpServers":[],"private-secret":1}"#,
        r#"{"mcpServers":{},"mcpServers":{"x":{"command":"private-secret"}}}"#,
    ] {
        fs::write(&target, invalid).unwrap();
        for op in ["list", "show", "use"] {
            let output = run(
                op,
                if op == "use" { Some(&stack) } else { None },
                &target,
                if op == "use" { &["-y"] } else { &[] },
            );
            assert!(!output.status.success());
            assert!(!String::from_utf8_lossy(&output.stderr).contains("private-secret"));
            assert_eq!(fs::read_to_string(&target).unwrap(), invalid);
        }
    }
}

#[test]
fn desktop_default_path_is_separate_from_claude_code_and_supports_explicit_override() {
    let dir = tempfile::tempdir().unwrap();
    let code = dir.path().join(".claude.json");
    fs::write(
        &code,
        r#"{"mcpServers":{"code-only":{"command":"code-tool"}}}"#,
    )
    .unwrap();
    let default = if cfg!(target_os = "macos") {
        dir.path()
            .join("Library/Application Support/Claude/claude_desktop_config.json")
    } else {
        dir.path().join("roaming/Claude/claude_desktop_config.json")
    };
    fs::create_dir_all(default.parent().unwrap()).unwrap();
    fs::write(
        &default,
        r#"{"mcpServers":{"desktop-only":{"command":"desktop-tool"}}}"#,
    )
    .unwrap();
    let mut cmd = command();
    cmd.env("HOME", dir.path())
        .env("USERPROFILE", dir.path())
        .env("APPDATA", dir.path().join("roaming"));
    let output = cmd.arg("list").output().unwrap();
    if cfg!(any(target_os = "macos", windows)) {
        let listing = success(output);
        assert!(listing.contains("desktop-only"));
        assert!(!listing.contains("code-only"));
    } else {
        assert_eq!(output.status.code(), Some(5));
        assert!(String::from_utf8_lossy(&output.stderr).contains("--config"));
    }
    assert!(success(run("list", None, &default, &[])).contains("desktop-only"));
    assert!(fs::read_to_string(code).unwrap().contains("code-only"));
}

#[test]
fn desktop_schema_and_help_only_advertise_supported_options() {
    let schema = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .arg("--schema")
        .output()
        .unwrap();
    assert!(schema.status.success());
    let schema: Value = serde_json::from_slice(&schema.stdout).unwrap();
    let desktop = schema["command"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|cmd| cmd["name"] == "claude-desktop")
        .unwrap();
    for operation in ["list", "show", "export", "diff", "import", "use"] {
        let entry = desktop["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|cmd| cmd["name"] == operation)
            .unwrap();
        let options = entry["options"].to_string();
        for unsupported in [
            "--include-permissions",
            "--permissions-config",
            "--include-workspace",
            "--all",
        ] {
            assert!(!options.contains(unsupported));
        }
        let help = success(command().args([operation, "--help"]).output().unwrap());
        assert!(help.contains("--config"));
        assert!(!help.contains("Claude Code"));
    }
}

#[test]
fn desktop_resolves_paths_and_secrets_without_starting_servers() {
    let dir = tempfile::tempdir().unwrap();
    let stacks = dir.path().join("stacks");
    fs::create_dir(&stacks).unwrap();
    let stack = stacks.join("local.yaml");
    let target = dir.path().join("desktop.json");
    fs::write(&stack, "schema_version: 1\nservers:\n  local:\n    transport:\n      type: stdio\n      command: ./tools/nonexistent-server\n      args: [--token, {value_env: MCPSTACK_DESKTOP_FIXTURE_TOKEN}]\n      env: {API_KEY: {value_env: MCPSTACK_DESKTOP_FIXTURE_TOKEN}}\n").unwrap();
    let mut cmd = command();
    cmd.args(["use"])
        .arg(&stack)
        .arg("--config")
        .arg(&target)
        .arg("--dry-run")
        .current_dir(dir.path())
        .env("MCPSTACK_DESKTOP_FIXTURE_TOKEN", "fixture-secret-value");
    let preview = success(cmd.output().unwrap());
    assert!(!preview.contains("fixture-secret-value"));
    assert!(!target.exists());
    let output = command()
        .arg("use")
        .arg(&stack)
        .arg("--config")
        .arg(&target)
        .arg("-y")
        .current_dir(dir.path())
        .env("MCPSTACK_DESKTOP_FIXTURE_TOKEN", "fixture-secret-value")
        .output()
        .unwrap();
    success(output);
    let server = &config(&target)["mcpServers"]["local"];
    assert_eq!(
        PathBuf::from(server["command"].as_str().unwrap()),
        fs::canonicalize(&stacks)
            .unwrap()
            .join("tools/nonexistent-server")
    );
    assert_eq!(server["args"][1], "fixture-secret-value");
    assert_eq!(server["env"]["API_KEY"], "fixture-secret-value");
    assert!(!success(run("show", None, &target, &[])).contains("fixture-secret-value"));
}

#[cfg(unix)]
#[test]
fn desktop_rejects_symlink_configuration_without_touching_its_target() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real.json");
    let link = dir.path().join("desktop.json");
    let stack = dir.path().join("stack.yaml");
    fs::write(&real, r#"{"mcpServers":{}}"#).unwrap();
    fs::write(
        &stack,
        "schema_version: 1\nservers:\n  new:\n    config: {command: example}\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    for operation in ["import", "use"] {
        let output = run(operation, Some(&stack), &link, &["-y"]);
        assert!(!output.status.success());
        assert_eq!(fs::read_to_string(&real).unwrap(), r#"{"mcpServers":{}}"#);
        assert!(!dir.path().join("desktop.json.~1~").exists());
    }
}

#[cfg(windows)]
#[test]
fn desktop_locked_config_preserves_original_and_reports_write_failure() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("desktop.json");
    let stack = dir.path().join("stack.yaml");
    let original = r#"{"mcpServers":{"old":{"command":"old-tool"}}}"#;
    fs::write(&target, original).unwrap();
    fs::write(
        &stack,
        "schema_version: 1\nservers:\n  new:\n    config: {command: example}\n",
    )
    .unwrap();
    let _locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&target)
        .unwrap();
    let output = run("use", Some(&stack), &target, &["-y"]);
    assert_eq!(output.status.code(), Some(13), "{output:?}");
    assert_eq!(fs::read_to_string(&target).unwrap(), original);
    assert_eq!(
        fs::read_to_string(dir.path().join("desktop.json.~1~")).unwrap(),
        original
    );
}
