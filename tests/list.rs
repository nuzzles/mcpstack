use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(client: &str, config: &Path) -> Output {
    run_with_flags(client, config, &[])
}

fn run_with_flags(client: &str, config: &Path, flags: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args([client, "list", "--config"])
        .arg(config)
        .args(flags)
        .env_remove("RUST_LOG")
        .env_remove("MCPSTACK_COLOR")
        .env("PATH", "")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn success(output: &Output) -> String {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn list_show_and_export_share_default_filters_and_individual_flags() {
    let names = [
        "ordinary",
        "computer-use",
        "node_repl",
        "workspace",
        "claude-in-chrome",
        "Claude Preview",
        "Claude Browser",
    ];
    let dir = tempfile::tempdir().unwrap();
    for client in ["codex", "claude"] {
        let config = dir.path().join(if client == "codex" {
            "config.toml"
        } else {
            "claude.json"
        });
        let document = if client == "codex" {
            names
                .iter()
                .map(|name| format!("[mcp_servers.{name:?}]\ncommand='tool'\n"))
                .collect::<String>()
        } else {
            let servers: serde_json::Map<_, _> = names
                .iter()
                .map(|name| ((*name).into(), serde_json::json!({"command":"tool"})))
                .collect();
            serde_json::json!({"mcpServers":servers}).to_string()
        };
        std::fs::write(&config, &document).unwrap();
        let default: BTreeSet<String> = if client == "codex" {
            [
                "ordinary",
                "workspace",
                "claude-in-chrome",
                "Claude Preview",
                "Claude Browser",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect()
        } else {
            ["ordinary", "node_repl"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        };
        let mut cases = vec![
            (None, None),
            (Some("--all"), None),
            (Some("--allow-computer-use"), Some("computer-use")),
        ];
        if client == "codex" {
            cases.push((Some("--allow-node-repl"), Some("node_repl")));
        } else {
            cases.extend([
                (Some("--allow-workspace"), Some("workspace")),
                (Some("--allow-claude-in-chrome"), Some("claude-in-chrome")),
                (Some("--allow-claude-preview"), Some("Claude Preview")),
                (Some("--allow-claude-browser"), Some("Claude Browser")),
            ]);
        }
        for (flag, included) in cases {
            let mut expected = default.clone();
            if flag == Some("--all") {
                expected = names.into_iter().map(str::to_owned).collect();
            }
            if let Some(name) = included {
                expected.insert(name.to_owned());
            }
            for operation in ["list", "show", "export"] {
                let destination = dir.path().join("stack.yml");
                let mut command = Command::new(env!("CARGO_BIN_EXE_mcpstack"));
                command
                    .args([client, operation, "--config"])
                    .arg(&config)
                    .env_remove("RUST_LOG")
                    .env_remove("MCPSTACK_COLOR")
                    .stdin(Stdio::null());
                if operation == "export" {
                    command.arg(&destination);
                }
                if let Some(flag) = flag {
                    command.arg(flag);
                }
                let output = success(&command.output().unwrap());
                let actual: BTreeSet<String> = if operation == "list" {
                    output
                        .lines()
                        .skip(1)
                        .map(|line| line.split("  ").next().unwrap().to_owned())
                        .collect()
                } else {
                    let yaml = if operation == "show" {
                        output
                    } else {
                        std::fs::read_to_string(&destination).unwrap()
                    };
                    let stack: serde_json::Value = yaml_serde::from_str(&yaml).unwrap();
                    stack["servers"]
                        .as_object()
                        .unwrap()
                        .keys()
                        .cloned()
                        .collect()
                };
                assert_eq!(actual, expected, "{client} {operation} {flag:?}");
            }
        }
        assert_eq!(std::fs::read_to_string(config).unwrap(), document);
    }
}

#[test]
fn list_filters_before_validation_and_rejects_other_clients_flags() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    for (client, document, foreign_flag) in [
        (
            "codex",
            "mcp_servers.computer-use='fixture-secret'",
            "--allow-workspace",
        ),
        (
            "claude",
            "{\"mcpServers\":{\"computer-use\":\"fixture-secret\"}}",
            "--allow-node-repl",
        ),
    ] {
        std::fs::write(&config, document).unwrap();
        assert_eq!(
            success(&run(client, &config)),
            "No MCP servers configured.\n"
        );
        for flag in ["--all", "--allow-computer-use"] {
            let output = run_with_flags(client, &config, &[flag]);
            assert_eq!(output.status.code(), Some(5));
            assert!(output.stdout.is_empty());
            assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-secret"));
        }
        let output = run_with_flags(client, &config, &[foreign_flag]);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn codex_lists_all_servers_in_order_without_exposing_credentials_or_changing_files() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let original = r#"
model = 'unrelated'
[mcp_servers.zebra]
url = 'https://user:fixture-secret@example.com/mcp?api_key=query-secret#fragment-secret'
enabled = false
http_headers = { Authorization = 'header-secret' }
[mcp_servers.alpha]
command = 'mcpstack-nonexistent-fixture'
args = ['--token', 'argument-secret']
env = { TOKEN = 'env-secret' }
future_setting = 'ignored-secret'
[mcp_servers.node_repl]
command = 'node'
[mcp_servers.computer-use]
command = 'computer'
"#;
    std::fs::write(&config, original).unwrap();
    let output = success(&run_with_flags("codex", &config, &["--all"]));
    let lines: Vec<_> = output
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .collect();
    assert_eq!(
        lines[1],
        ["alpha", "stdio", "enabled", "mcpstack-nonexistent-fixture"]
    );
    assert_eq!(lines[2], ["computer-use", "stdio", "enabled", "computer"]);
    assert_eq!(lines[3], ["node_repl", "stdio", "enabled", "node"]);
    assert_eq!(
        lines[4],
        ["zebra", "http", "disabled", "https://example.com/mcp"]
    );
    for secret in [
        "fixture-secret",
        "query-secret",
        "fragment-secret",
        "header-secret",
        "argument-secret",
        "env-secret",
        "ignored-secret",
    ] {
        assert!(!output.contains(secret), "{output}");
    }
    assert_eq!(
        success(&run_with_flags("codex", &config, &["--all"])),
        output
    );
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn claude_lists_user_scope_transports_and_reserved_names_without_claiming_enabled_state() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    let original = serde_json::json!({
        "theme": "dark",
        "projects": {"example": {"mcpServers": {"project-only": {"command": "hidden"}}}},
        "mcpServers": {
            "stdio": {"command": "mcpstack-nonexistent-fixture", "args": ["arg-secret"], "env": {"KEY": "env-secret"}},
            "explicit": {"type": "stdio", "command": "tool"},
            "http": {"type": "http", "url": "https://user:fixture-secret@example.com/mcp?secret=query-secret#fragment-secret", "headers": {"Authorization": "header-secret"}},
            "sse": {"type": "sse", "url": "https://example.com/events"},
            "computer-use": {"type": "http", "url": "http://localhost:1000/mcp"}
        }
    }).to_string();
    std::fs::write(&config, &original).unwrap();
    let output = success(&run_with_flags("claude", &config, &["--all"]));
    let lines: Vec<_> = output
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .collect();
    assert_eq!(
        lines[1],
        [
            "computer-use",
            "http",
            "ignored",
            "http://localhost:1000/mcp"
        ]
    );
    assert_eq!(lines[2], ["explicit", "stdio", "configured", "tool"]);
    assert_eq!(
        lines[3],
        ["http", "http", "configured", "https://example.com/mcp"]
    );
    assert_eq!(
        lines[4],
        ["sse", "sse", "configured", "https://example.com/events"]
    );
    assert_eq!(
        lines[5],
        [
            "stdio",
            "stdio",
            "configured",
            "mcpstack-nonexistent-fixture"
        ]
    );
    assert_eq!(lines.len(), 6);
    assert!(!output.contains("Configured:"));
    assert!(!output.contains("permission"));
    for value in ["secret", "project-only", "theme"] {
        assert!(!output.contains(value), "{output}");
    }
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn empty_inventory_and_missing_configs_are_distinct() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    for (client, empty) in [
        ("codex", "model='irrelevant'"),
        ("claude", "{}"),
        ("codex", "[mcp_servers]"),
        ("claude", "{\"mcpServers\":{}}"),
        ("codex", ""),
        ("claude", ""),
    ] {
        std::fs::write(&config, empty).unwrap();
        assert_eq!(
            success(&run(client, &config)),
            "No MCP servers configured.\n"
        );
    }
    for client in ["codex", "claude"] {
        let output = run(client, &dir.path().join("missing"));
        assert_eq!(output.status.code(), Some(5));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("CONFIG_READ_ERROR"));
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn invalid_configs_fail_before_any_rows_are_printed() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    for (client, invalid) in [
        ("codex", "broken='fixture-secret"),
        ("codex", "mcp_servers='fixture-secret'"),
        (
            "codex",
            "[mcp_servers.a]\ncommand='tool'\n[mcp_servers.z]\nenabled='fixture-secret'\ncommand='tool'",
        ),
        (
            "codex",
            "[mcp_servers.a]\ncommand='tool'\nurl='https://example.com'",
        ),
        (
            "claude",
            "{\"mcpServers\":{\"x\":{\"command\":\"tool\"},\"x\":{\"command\":\"fixture-secret\"}}}",
        ),
        (
            "claude",
            "{\"mcpServers\":{\"a\":{\"command\":\"tool\"},\"z\":{\"type\":\"http\",\"url\":\"fixture-secret\"}}}",
        ),
        ("claude", "[]"),
        (
            "claude",
            "{\"mcpServers\":{\"bad\":{\"type\":123,\"command\":\"fixture-secret\"}}}",
        ),
    ] {
        std::fs::write(&config, invalid).unwrap();
        let output = run(client, &config);
        assert_eq!(output.status.code(), Some(5), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(diagnostic.contains("CONFIG_READ_ERROR"));
        assert!(!diagnostic.contains("fixture-secret"));
        assert_eq!(std::fs::read_to_string(&config).unwrap(), invalid);
    }
}

#[test]
fn inventory_escapes_terminal_control_characters() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude.json");
    std::fs::write(
        &config,
        serde_json::json!({"mcpServers": {"a\n\u{1b}[31m": {"command": "tool\targument"}}})
            .to_string(),
    )
    .unwrap();
    let output = success(&run("claude", &config));
    assert!(output.contains("a\\n\\u{1b}[31m"), "{output}");
    assert!(output.contains("tool\\targument"));
    assert!(!output.contains('\u{1b}'));
}

#[test]
fn codex_list_uses_codex_home_and_both_commands_are_in_the_cli_schema() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config.toml"),
        "[mcp_servers.from_home]\ncommand='fixture'",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args(["codex", "list"])
        .env("CODEX_HOME", dir.path())
        .env_remove("RUST_LOG")
        .env_remove("MCPSTACK_COLOR")
        .output()
        .unwrap();
    assert!(success(&output).contains("from_home"));
    let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .arg("--schema")
        .env_remove("RUST_LOG")
        .output()
        .unwrap();
    let schema: serde_json::Value = serde_json::from_str(&success(&output)).unwrap();
    for client in ["codex", "claude"] {
        let client = schema["command"]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == client)
            .unwrap();
        let list = client["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "list")
            .unwrap();
        assert!(list["arguments"].as_array().unwrap().is_empty());
        assert!(
            list["options"]
                .as_array()
                .unwrap()
                .iter()
                .any(|option| option["long"] == "--config")
        );
    }
}
