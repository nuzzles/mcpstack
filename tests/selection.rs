use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(client: &str, operation: &str, config: &Path, file: &Path, flags: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcpstack"));
    command.args(["--non-interactive", "--color", "never", client, operation]);
    if matches!(operation, "export" | "import" | "diff" | "use") {
        command.arg(file);
    }
    if operation == "export" {
        command.arg("--overwrite");
    }
    command
        .arg("--config")
        .arg(config)
        .args(flags)
        .env_remove("RUST_LOG")
        .env_remove("MCPSTACK_SELECTION_MISSING_SECRET")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn success(output: Output) -> String {
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

fn config_text(client: &str) -> &'static str {
    if client == "codex" {
        "model='unrelated'\n[mcp_servers.alpha]\ncommand='old-alpha'\n[mcp_servers.beta]\ncommand='old-beta'\n[mcp_servers.computer-use]\ncommand='internal'\n"
    } else {
        r#"{"theme":"unrelated","mcpServers":{"alpha":{"command":"old-alpha"},"beta":{"command":"old-beta"},"computer-use":{"command":"internal"}}}"#
    }
}

#[test]
fn config_selection_is_shared_ordered_and_deduplicated() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    let file = dir.path().join("stack.yml");
    for client in ["codex", "claude"] {
        std::fs::write(&config, config_text(client)).unwrap();
        for operation in ["list", "show", "export"] {
            for (flags, expected) in [
                (
                    vec!["--server", "beta", "--server", "alpha", "--server", "beta"],
                    vec!["alpha", "beta"],
                ),
                (vec!["--all", "--server", "alpha"], vec!["alpha"]),
                (
                    vec!["--include-computer-use", "--server", "computer-use"],
                    vec!["computer-use"],
                ),
            ] {
                let output = success(run(client, operation, &config, &file, &flags));
                let actual: Vec<String> = if operation == "list" {
                    output
                        .lines()
                        .skip(1)
                        .map(|line| line.split_whitespace().next().unwrap().to_owned())
                        .collect()
                } else {
                    let yaml = if operation == "export" {
                        std::fs::read_to_string(&file).unwrap()
                    } else {
                        output
                    };
                    let stack: serde_json::Value = yaml_serde::from_str(&yaml).unwrap();
                    stack["servers"]
                        .as_object()
                        .unwrap()
                        .keys()
                        .cloned()
                        .collect()
                };
                assert_eq!(actual, expected, "{client} {operation} {flags:?}");
            }
        }
        assert_eq!(
            std::fs::read_to_string(&config).unwrap(),
            config_text(client)
        );
    }
}

#[test]
fn selection_precedes_native_server_validation() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    let file = dir.path().join("stack.yml");
    for (client, document) in [
        (
            "codex",
            "[mcp_servers.alpha]\ncommand='tool'\n[mcp_servers.invalid]\nunknown_field='fixture-secret'",
        ),
        (
            "claude",
            r#"{"mcpServers":{"alpha":{"command":"tool"},"invalid":{"unknown_field":"fixture-secret"}}}"#,
        ),
    ] {
        std::fs::write(&config, document).unwrap();
        for operation in ["list", "show", "export"] {
            let output = success(run(
                client,
                operation,
                &config,
                &file,
                &["--server", "alpha"],
            ));
            assert!(!output.contains("fixture-secret"));
            assert!(!run(client, operation, &config, &file, &[]).status.success());
        }
    }
}

#[test]
fn unknown_or_excluded_selections_fail_without_output_or_writes() {
    for client in ["codex", "claude"] {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let file = dir.path().join("stack.yml");
        let stack = "schema_version: 1\nservers:\n  alpha:\n    config: {command: tool}\n";
        std::fs::write(&config, config_text(client)).unwrap();
        std::fs::write(&file, stack).unwrap();
        for operation in ["list", "show", "export", "diff", "import"] {
            let output = run(
                client,
                operation,
                &config,
                &file,
                &["--server", "alpha", "--server", "Alpha"],
            );
            assert_eq!(output.status.code(), Some(2), "{output:?}");
            assert!(output.stdout.is_empty(), "{output:?}");
            let error = String::from_utf8(output.stderr).unwrap();
            assert!(
                error.contains("Unknown MCP server selection(s): \"Alpha\""),
                "{error}"
            );
        }
        // Automatic approval must not create a backup for an unknown selection.
        assert_eq!(
            run(
                client,
                "import",
                &config,
                &file,
                &["-y", "--server", "missing"]
            )
            .status
            .code(),
            Some(2)
        );
        for operation in ["list", "show", "export"] {
            let output = run(
                client,
                operation,
                &config,
                &file,
                &["--server", "computer-use"],
            );
            assert_eq!(output.status.code(), Some(2), "{output:?}");
            assert!(output.stdout.is_empty());
            assert!(
                String::from_utf8(output.stderr)
                    .unwrap()
                    .contains("excluded by default")
            );
        }
        assert_eq!(std::fs::read_to_string(&file).unwrap(), stack);
        assert_eq!(
            std::fs::read_to_string(&config).unwrap(),
            config_text(client)
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
        for empty in if client == "codex" {
            ["", "[mcp_servers]"]
        } else {
            ["{}", r#"{"mcpServers":{}}"#]
        } {
            std::fs::write(&config, empty).unwrap();
            for operation in ["list", "show", "export"] {
                assert_eq!(
                    run(client, operation, &config, &file, &["--server", "missing"])
                        .status
                        .code(),
                    Some(2)
                );
            }
        }
    }
}

#[test]
fn selected_import_and_diff_skip_other_secrets_and_unsupported_fields() {
    for client in ["codex", "claude"] {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let file = dir.path().join("stack.yml");
        std::fs::write(&config, config_text(client)).unwrap();
        std::fs::write(&file, "schema_version: 1\nservers:\n  alpha:\n    config: {command: new-alpha}\n  beta:\n    config: {command: new-beta, env: {TOKEN: {$env: MCPSTACK_SELECTION_MISSING_SECRET}}}\n  incompatible:\n    config: {command: tool, unsupported_setting: true}\n").unwrap();
        for operation in ["diff", "import"] {
            let flags = if operation == "import" {
                vec!["--server", "alpha", "--dry-run", "-y"]
            } else {
                vec!["--server", "alpha"]
            };
            let preview = success(run(client, operation, &config, &file, &flags));
            assert!(preview.contains("alpha"));
            assert!(!preview.contains("new-beta"));
            assert!(!preview.contains("incompatible"));
            assert_eq!(
                std::fs::read_to_string(&config).unwrap(),
                config_text(client)
            );
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
        }
        success(run(
            client,
            "import",
            &config,
            &file,
            &["--server", "alpha", "--server", "alpha", "-y"],
        ));
        let applied = std::fs::read_to_string(&config).unwrap();
        assert!(applied.contains("new-alpha"));
        assert!(applied.contains("old-beta"));
        assert!(applied.contains("internal"));
        assert!(applied.contains("unrelated"));
        assert!(!applied.contains("new-beta"));
        assert!(!applied.contains("incompatible"));
        success(run(
            client,
            "import",
            &config,
            &file,
            &["--server", "alpha", "-y"],
        ));
        assert_eq!(std::fs::read_to_string(&config).unwrap(), applied);
        let unchanged = success(run(client, "diff", &config, &file, &["--server", "alpha"]));
        assert!(!unchanged.contains("@@"), "{unchanged}");
    }
}

#[test]
fn use_rejects_selection_and_schema_advertises_only_supported_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .arg("--schema")
        .output()
        .unwrap();
    let schema: serde_json::Value = serde_json::from_str(&success(output)).unwrap();
    for client in ["codex", "claude"] {
        let client_schema = schema["command"]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == client)
            .unwrap();
        for operation in client_schema["commands"].as_array().unwrap() {
            let selection = operation["options"]
                .as_array()
                .unwrap()
                .iter()
                .find(|option| option["long"] == "--server");
            assert_eq!(
                selection.is_some(),
                matches!(
                    operation["name"].as_str(),
                    Some("list" | "show" | "export" | "import" | "diff")
                ),
                "{operation}"
            );
        }
        let output = run(
            client,
            "use",
            Path::new("missing-config"),
            Path::new("missing.yml"),
            &["--server", "alpha"],
        );
        assert_eq!(output.status.code(), Some(2));
    }
}

#[test]
fn selected_permissions_export_and_restore_without_changing_other_servers() {
    for client in ["codex", "claude"] {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let target = dir.path().join("target");
        let file = dir.path().join("stack.yml");
        let source_settings = dir.path().join("source-settings.json");
        let target_settings = dir.path().join("target-settings.json");
        let mut show_flags = vec!["--include-permissions"];
        let mut import_flags = vec!["--include-permissions", "--server", "alpha", "-y"];
        if client == "codex" {
            std::fs::write(&source, "[mcp_servers.alpha]\ncommand='new-alpha'\ndefault_tools_approval_mode='approve'\n[mcp_servers.beta]\ncommand='new-beta'\ndefault_tools_approval_mode='approve'\n").unwrap();
            std::fs::write(&target, "[mcp_servers.alpha]\ncommand='old-alpha'\ndefault_tools_approval_mode='prompt'\n[mcp_servers.beta]\ncommand='old-beta'\ndefault_tools_approval_mode='prompt'\n").unwrap();
        } else {
            std::fs::write(
                &source,
                r#"{"mcpServers":{"alpha":{"command":"new-alpha"},"beta":{"command":"new-beta"}}}"#,
            )
            .unwrap();
            std::fs::write(&target, config_text(client)).unwrap();
            std::fs::write(
                &source_settings,
                r#"{"permissions":{"allow":["mcp__alpha__read","mcp__beta__read"]}}"#,
            )
            .unwrap();
            std::fs::write(
                &target_settings,
                r#"{"permissions":{"allow":["Bash","mcp__alpha__old","mcp__beta__old"]}}"#,
            )
            .unwrap();
            show_flags.extend(["--permissions-config", source_settings.to_str().unwrap()]);
            import_flags.extend(["--permissions-config", target_settings.to_str().unwrap()]);
        }
        let full_stack = success(run(client, "show", &source, &file, &show_flags));
        std::fs::write(&file, full_stack).unwrap();
        show_flags.extend(["--server", "alpha"]);
        let selected = success(run(client, "show", &source, &file, &show_flags));
        let selected: serde_json::Value = yaml_serde::from_str(&selected).unwrap();
        assert_eq!(selected["servers"].as_object().unwrap().len(), 1);
        assert!(selected["servers"]["alpha"]["permissions"].is_object());
        success(run(client, "import", &target, &file, &import_flags));
        let config = std::fs::read_to_string(&target).unwrap();
        if client == "codex" {
            let config: toml::Value = toml::from_str(&config).unwrap();
            assert_eq!(
                config["mcp_servers"]["alpha"]["default_tools_approval_mode"].as_str(),
                Some("approve")
            );
            assert_eq!(
                config["mcp_servers"]["beta"]["default_tools_approval_mode"].as_str(),
                Some("prompt")
            );
            assert_eq!(
                config["mcp_servers"]["beta"]["command"].as_str(),
                Some("old-beta")
            );
        } else {
            let config: serde_json::Value = serde_json::from_str(&config).unwrap();
            assert_eq!(config["mcpServers"]["beta"]["command"], "old-beta");
            let settings: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&target_settings).unwrap()).unwrap();
            let rules = settings["permissions"]["allow"].as_array().unwrap();
            assert!(rules.contains(&serde_json::json!("mcp__alpha__read")));
            assert!(rules.contains(&serde_json::json!("mcp__beta__old")));
            assert!(rules.contains(&serde_json::json!("Bash")));
            assert!(!rules.contains(&serde_json::json!("mcp__alpha__old")));
            assert!(!rules.contains(&serde_json::json!("mcp__beta__read")));
        }
    }
}
