use std::path::Path;
use std::process::{Command, Output, Stdio};

fn export(client: &str, config: &Path, destination: &Path, flags: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args([client, "export"])
        .arg(destination)
        .arg("--config")
        .arg(config)
        .args(flags)
        .env_remove("RUST_LOG")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn source(client: &str) -> &'static str {
    if client == "codex" {
        "[mcp_servers.example]\ncommand='tool'\n"
    } else {
        r#"{"mcpServers":{"example":{"command":"tool"}}}"#
    }
}

#[test]
fn existing_exports_require_explicit_overwrite_without_a_terminal() {
    for client in ["codex", "claude"] {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let destination = dir.path().join("stack.yml");
        std::fs::write(&config, source(client)).unwrap();
        // Missing files need no confirmation, including in noninteractive mode.
        let created = export(client, &config, &destination, &["--non-interactive"]);
        assert!(created.status.success(), "{created:?}");
        let expected = std::fs::read(&destination).unwrap();
        // Empty files and identical exports still count as existing files.
        for original in [
            b"private existing stack".as_slice(),
            b"",
            expected.as_slice(),
        ] {
            std::fs::write(&destination, original).unwrap();
            let modified = std::fs::metadata(&destination).unwrap().modified().unwrap();
            for flags in [
                vec![],
                vec!["--non-interactive"],
                vec!["--expose-secrets", "--non-interactive"],
            ] {
                let output = export(client, &config, &destination, &flags);
                assert_eq!(output.status.code(), Some(2), "{output:?}");
                assert!(output.stdout.is_empty());
                let diagnostic = String::from_utf8_lossy(&output.stderr);
                assert!(diagnostic.contains("already exists"));
                assert!(diagnostic.contains("--overwrite"));
                assert!(!diagnostic.contains("private existing stack"));
                assert_eq!(std::fs::read(&destination).unwrap(), original);
                assert_eq!(
                    std::fs::metadata(&destination).unwrap().modified().unwrap(),
                    modified
                );
            }
            for flags in [
                vec!["--overwrite"],
                vec!["--overwrite", "--non-interactive"],
            ] {
                let output = export(client, &config, &destination, &flags);
                assert!(output.status.success(), "{output:?}");
                assert_eq!(std::fs::read(&destination).unwrap(), expected);
            }
        }
        assert_eq!(std::fs::read_to_string(&config).unwrap(), source(client));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}

#[test]
fn overwrite_does_not_bypass_source_protection_or_generation_errors() {
    for client in ["codex", "claude"] {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("source.yaml");
        let destination = dir.path().join("stack.yml");
        std::fs::write(&config, source(client)).unwrap();
        let output = export(
            client,
            &config,
            &config,
            &["--overwrite", "--non-interactive"],
        );
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("must differ"));
        assert_eq!(std::fs::read_to_string(&config).unwrap(), source(client));

        std::fs::write(&destination, "original stack").unwrap();
        std::fs::write(&config, "[malformed private-fixture").unwrap();
        let output = export(
            client,
            &config,
            &destination,
            &["--overwrite", "--non-interactive"],
        );
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private-fixture"));
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "original stack"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}

fn reference_command(
    client: &str,
    operation: &str,
    config: &Path,
    stack: Option<&Path>,
    base: &str,
) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcpstack"));
    command.args([
        "--non-interactive",
        "--skip-version-check",
        client,
        operation,
    ]);
    if let Some(stack) = stack {
        command.arg(stack);
    }
    command
        .arg("--config")
        .arg(config)
        .env_remove("RUST_LOG")
        .env("NO_COLOR", "1")
        .env(base, "runtime-fixture-secret")
        .env(format!("{base}_2"), "second-runtime-fixture-secret")
        .env_remove(format!("{base}_3"))
        .stdin(Stdio::null());
    command
}

#[test]
fn exported_literals_never_reuse_runtime_header_variables() {
    for client in ["claude", "codex"] {
        let base = if client == "claude" {
            "MCPSTACK_DEMO_HEADERS_X_API_KEY"
        } else {
            "MCPSTACK_DEMO_HTTP_HEADERS_X_API_KEY"
        };
        // Bindings in a later server must reserve both the base and its suffix
        // before the earlier server's literal is masked.
        let original = if client == "claude" {
            serde_json::json!({"theme":"keep", "mcpServers":{
                "demo":{"type":"http","url":"https://example.com/mcp",
                    "headers":{"X-Api-Key":"literal-fixture-secret"}},
                "later":{"type":"http","url":"https://example.com/other",
                    "headers":{"Authorization":format!("${{{base}}}"),
                        "X-Other":format!("${{{base}_2}}")}}
            }})
            .to_string()
        } else {
            format!(
                "# keep this comment\nmodel='keep'\n\
                [mcp_servers.demo]\nurl='https://example.com/mcp'\n\
                [mcp_servers.demo.http_headers]\nX-Api-Key='literal-fixture-secret'\n\
                [mcp_servers.later]\nurl='https://example.com/other'\n\
                [mcp_servers.later.env_http_headers]\nAuthorization='{base}'\nX-Other='{base}_2'\n"
            )
        };
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let stack = dir.path().join("stack.yaml");
        std::fs::write(&config, &original).unwrap();
        let exported = reference_command(client, "export", &config, Some(&stack), base)
            .output()
            .unwrap();
        assert!(exported.status.success(), "{exported:?}");
        let yaml = std::fs::read_to_string(&stack).unwrap();
        let value: serde_json::Value = yaml_serde::from_str(&yaml).unwrap();
        let reference = format!("{base}_3");
        assert_eq!(
            value["servers"]["demo"]["transport"]["headers"]["X-Api-Key"]["value_env"],
            reference
        );
        assert_eq!(
            value["servers"]["later"]["transport"]["headers"]["Authorization"]["env"],
            base
        );
        assert!(!yaml.contains("fixture-secret"));
        // Export must not depend on values or availability in the environment.
        let shown = reference_command(client, "show", &config, None, base)
            .env_remove(base)
            .env_remove(format!("{base}_2"))
            .output()
            .unwrap();
        assert!(shown.status.success(), "{shown:?}");
        assert_eq!(String::from_utf8(shown.stdout).unwrap(), yaml);

        for (operation, flags) in [
            ("diff", vec![]),
            ("import", vec!["--dry-run", "-y"]),
            ("use", vec!["--dry-run"]),
            ("import", vec!["-y"]),
            ("use", vec!["-y"]),
        ] {
            for supplied in [false, true] {
                let mut command = reference_command(client, operation, &config, Some(&stack), base);
                command.args(&flags);
                if supplied {
                    command.env(&reference, "literal-fixture-secret");
                }
                let result = command.output().unwrap();
                assert_eq!(result.status.success(), supplied, "{result:?}");
                if supplied {
                    assert!(String::from_utf8_lossy(&result.stdout).contains("No changes"));
                } else {
                    assert_eq!(result.status.code(), Some(10));
                }
                assert!(!String::from_utf8_lossy(&result.stdout).contains("fixture-secret"));
                assert!(!String::from_utf8_lossy(&result.stderr).contains("fixture-secret"));
                assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
                assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
            }
        }
    }
}

#[test]
fn exports_reserve_bearer_forwarded_and_claude_stdio_variables() {
    let base = "MCPSTACK_DEMO_ENV_API_TOKEN";
    for client in ["claude", "codex"] {
        let original = if client == "claude" {
            serde_json::json!({"mcpServers":{
                "demo":{"command":"example","env":{"API_TOKEN":"literal-fixture-secret"}},
                "later":{"command":"example","args":[format!("${{{base}}}")],
                    "env":{"API_TOKEN":format!("${{{base}_2}}")}}
            }})
            .to_string()
        } else {
            format!(
                "[mcp_servers.demo]\ncommand='example'\n\
                [mcp_servers.demo.env]\nAPI_TOKEN='literal-fixture-secret'\n\
                [mcp_servers.later]\nurl='https://example.com'\nbearer_token_env_var='{base}'\n\
                [mcp_servers.forwarded]\ncommand='example'\nenv_vars=['{base}_2']\n"
            )
        };
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let stack = dir.path().join("stack.yaml");
        std::fs::write(&config, &original).unwrap();
        let exported = reference_command(client, "export", &config, Some(&stack), base)
            .output()
            .unwrap();
        assert!(exported.status.success(), "{exported:?}");
        let yaml = std::fs::read_to_string(&stack).unwrap();
        let value: serde_json::Value = yaml_serde::from_str(&yaml).unwrap();
        assert_eq!(
            value["servers"]["demo"]["transport"]["env"]["API_TOKEN"]["value_env"],
            format!("{base}_3")
        );
        let applied = reference_command(client, "use", &config, Some(&stack), base)
            .arg("-y")
            .env(format!("{base}_3"), "literal-fixture-secret")
            .output()
            .unwrap();
        assert!(applied.status.success(), "{applied:?}");
        assert!(String::from_utf8_lossy(&applied.stdout).contains("No changes"));
        assert!(!String::from_utf8_lossy(&applied.stdout).contains("fixture-secret"));
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}
