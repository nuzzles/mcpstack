use std::process::{Command, Output, Stdio};

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcpstack"));
    command
        .args(["--color", "never", "--non-interactive"])
        .env_remove("RUST_LOG")
        .env_remove("MCPSTACK_COLOR")
        .env_remove("NO_COLOR")
        .stdin(Stdio::null());
    command
}

fn success(output: Output) -> String {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert!(!output.stdout.contains(&0x1b), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn fatal_diagnostics_survive_disabled_logs_without_exposing_input() {
    let dir = tempfile::tempdir().unwrap();
    let stack = dir.path().join("invalid.yml");
    std::fs::write(&stack, "fixture-secret: [unterminated").unwrap();
    for (before, after, filter) in [
        (vec!["--log", "off"], vec![], None),
        (vec![], vec!["--log", "off"], None),
        (vec![], vec![], Some("off")),
        (vec![], vec![], Some("error,mcpstack=off")),
        (vec!["--log", "error"], vec![], None),
    ] {
        let mut command = command();
        command.args(before).arg("validate").arg(&stack).args(after);
        if let Some(filter) = filter {
            command.env("RUST_LOG", filter);
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(4), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        let diagnostic = String::from_utf8(output.stderr).unwrap();
        assert_eq!(
            diagnostic.matches("INVALID_STACK:").count(),
            1,
            "{diagnostic}"
        );
        assert!(!diagnostic.contains("fixture-secret"), "{diagnostic}");
        assert!(!diagnostic.contains('\x1b'), "{diagnostic}");
    }
}

#[test]
fn both_clients_report_no_op_targets_and_dry_run_outcomes_without_writes() {
    for (client, original) in [
        ("codex", "[mcp_servers.example]\ncommand = 'tool'\n"),
        ("claude", r#"{"mcpServers":{"example":{"command":"tool"}}}"#),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let stack = dir.path().join("stack.yml");
        std::fs::write(&config, original).unwrap();
        std::fs::write(&stack, "schema_version: 1\nservers:\n  example:\n    transport: {type: stdio, command: tool}\n").unwrap();
        for (operation, flags) in [
            ("diff", vec![]),
            ("import", vec!["-y"]),
            ("use", vec![]),
            ("import", vec!["--dry-run", "-y"]),
            ("use", vec!["--dry-run"]),
        ] {
            let output = success(
                command()
                    .args([client, operation])
                    .arg(&stack)
                    .arg("--config")
                    .arg(&config)
                    .args(&flags)
                    .output()
                    .unwrap(),
            );
            let mut expected = format!(
                "Target config: {}\nNo changes; requested configuration is already in place.\n",
                config.to_string_lossy().escape_debug()
            );
            if flags.contains(&"--dry-run") {
                expected.push_str("Dry run; no configuration files were changed.\n");
            }
            assert_eq!(output, expected, "{client} {operation} {flags:?}");
            assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
        }
    }
}

#[test]
fn import_counts_changed_servers_and_distinguishes_preview_from_commit() {
    for (client, original) in [
        (
            "codex",
            "[mcp_servers.same]\ncommand = 'same'\n[mcp_servers.changed]\ncommand = 'old'\n",
        ),
        (
            "claude",
            r#"{"mcpServers":{"same":{"command":"same"},"changed":{"command":"old"}}}"#,
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let stack = dir.path().join("stack.yml");
        std::fs::write(&config, original).unwrap();
        std::fs::write(&stack, "schema_version: 1\nservers:\n  same:\n    transport: {type: stdio, command: same}\n  changed:\n    transport: {type: stdio, command: new}\n  added:\n    transport: {type: stdio, command: added}\n").unwrap();
        for dry_run in [true, false] {
            let mut command = command();
            command
                .args([client, "import"])
                .arg(&stack)
                .arg("--config")
                .arg(&config)
                .arg("-y");
            if dry_run {
                command.arg("--dry-run");
            }
            let output = success(command.output().unwrap());
            assert!(output.contains("  + Added: \"added\"\n"), "{output}");
            assert!(output.contains("  ~ Modified: \"changed\"\n"), "{output}");
            assert!(output.contains("  - Deleted: none\n"), "{output}");
            if dry_run {
                assert!(output.ends_with("Would import 2 MCP server(s); identical entries were unchanged.\nDry run; no configuration files were changed.\n"), "{output}");
                assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
                assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
            } else {
                assert!(
                    output
                        .ends_with("Imported 2 MCP server(s); identical entries were unchanged.\n"),
                    "{output}"
                );
                assert!(!output.contains("Would import"), "{output}");
                assert_eq!(
                    std::fs::read_to_string(dir.path().join("config.~1~")).unwrap(),
                    original
                );
            }
        }
    }
}

#[test]
fn permission_only_import_reports_settings_changes_without_server_imports() {
    let dir = tempfile::Builder::new()
        .prefix("import-Imported-")
        .tempdir()
        .unwrap();
    let config = dir.path().join("claude.json");
    let settings = dir.path().join("settings.json");
    let stack = dir.path().join("stack.yml");
    let original_config = r#"{"mcpServers":{"example":{"command":"tool"}}}"#;
    let original_settings = r#"{"permissions":{"ask":["mcp__example__read"]}}"#;
    std::fs::write(&config, original_config).unwrap();
    std::fs::write(&settings, original_settings).unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers:\n  example:\n    transport: {type: stdio, command: tool}\n    permissions: {allow: ['read']}\n").unwrap();
    for dry_run in [true, false] {
        let mut command = command();
        command
            .args(["claude", "import"])
            .arg(&stack)
            .arg("--config")
            .arg(&config)
            .args(["--include-permissions", "-y"]);
        if dry_run {
            command.arg("--dry-run");
        }
        let output = success(command.output().unwrap());
        assert!(
            output.contains(&format!(
                "Target settings: {}",
                settings.to_string_lossy().escape_debug()
            )),
            "{output}"
        );
        assert!(
            output.contains("\"mcp__example__read\" (ask -> allow)"),
            "{output}"
        );
        assert!(
            !output
                .lines()
                .any(|line| line.starts_with("Would import ") || line.starts_with("Imported ")),
            "{output}"
        );
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original_config);
        if dry_run {
            assert!(output.ends_with("Would update MCP permission rules.\nDry run; no configuration files were changed.\n"), "{output}");
            assert_eq!(
                std::fs::read_to_string(&settings).unwrap(),
                original_settings
            );
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
        } else {
            assert!(
                output.ends_with("Updated MCP permission rules.\n"),
                "{output}"
            );
            let value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
            assert_eq!(
                value["permissions"]["allow"],
                serde_json::json!(["mcp__example__read"])
            );
            assert!(!dir.path().join("claude.json.~1~").exists());
            assert_eq!(
                std::fs::read_to_string(dir.path().join("settings.json.~1~")).unwrap(),
                original_settings
            );
        }
    }
}

#[test]
#[cfg(unix)]
fn export_and_validation_report_absolute_escaped_paths() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.json");
    let filename = "stack\n\x1b[31m.yml";
    let stack = dir.path().join(filename);
    std::fs::write(&config, r#"{"mcpServers":{}}"#).unwrap();
    let output = success(
        command()
            .current_dir(dir.path())
            .args(["claude", "export", filename, "--config"])
            .arg(&config)
            .output()
            .unwrap(),
    );
    // Resolve the temporary directory alias in the same way as the child's cwd.
    let absolute = std::fs::canonicalize(dir.path()).unwrap().join(filename);
    let path = absolute.to_string_lossy().escape_debug().to_string();
    assert_eq!(output, format!("Exported stack to {path}.\n"));
    assert!(stack.exists());
    let output = success(
        command()
            .current_dir(dir.path())
            .args(["validate", filename])
            .output()
            .unwrap(),
    );
    assert_eq!(
        output,
        format!("Stack file: {path}\nValid stack (schema 1, 0 server(s)).\n")
    );
}

#[test]
fn help_matches_each_client_and_describes_preview_approval() {
    for client in ["claude", "codex"] {
        let help = success(command().args([client, "use", "--help"]).output().unwrap());
        assert!(
            help.contains(&format!("mcpstack {client} use work.yml")),
            "{help}"
        );
        assert!(
            help.contains("without approval prompts or writes"),
            "{help}"
        );
        assert!(
            help.contains(if client == "claude" {
                "claude.json"
            } else {
                "config.toml"
            }),
            "{help}"
        );
        let help = success(
            command()
                .args([client, "import", "--help"])
                .output()
                .unwrap(),
        );
        assert!(
            help.contains("use --non-interactive -y for unattended previews"),
            "{help}"
        );
        assert!(
            help.contains("missing secrets may still prompt even with -y"),
            "{help}"
        );
        assert!(help.contains("Summarize changes"), "{help}");
    }
}
