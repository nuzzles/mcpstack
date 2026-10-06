use std::process::{Command, Output, Stdio};

use serde_json::Value;

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args(args)
        .env_remove("RUST_LOG")
        .env_remove("MCPSTACK_COLOR")
        .env_remove("NO_COLOR")
        .stdin(Stdio::null())
        .output()
        .expect("CLI should start")
}

fn json(output: &Output) -> Value {
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("stdout should contain one JSON document")
}

#[test]
fn schema_describes_the_executable_interface_and_is_deterministic() {
    let first = run(&["--schema"]);
    let second = run(&["--schema"]);
    assert!(first.status.success());
    assert_eq!(first.stdout, second.stdout);
    let schema = json(&first);
    assert_eq!(schema["schema_version"], 1);
    assert_eq!(schema["cli_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(schema["command"]["name"], "mcpstack");
    let commands = schema["command"]["commands"].as_array().unwrap();
    assert_eq!(commands.len(), 4); // validate, export, import, and generated help
    let validate = commands
        .iter()
        .find(|command| command["name"] == "validate")
        .unwrap();
    assert_eq!(validate["arguments"][0]["name"], "file");
    assert_eq!(validate["arguments"][0]["required"], true);
    assert_eq!(schema["command"]["args_conflicts_with_subcommands"], false);

    let help = run(&["--help"]);
    let help = String::from_utf8(help.stdout).unwrap();
    let options = schema["command"]["options"].as_array().unwrap();
    assert_eq!(options.len(), 9);
    let non_interactive = options
        .iter()
        .find(|option| option["name"] == "non_interactive")
        .unwrap();
    assert_eq!(non_interactive["long"], "--non-interactive");
    for option in options
        .iter()
        .filter(|option| matches!(option["name"].as_str(), Some("help" | "version" | "schema")))
    {
        let flag = option["long"].as_str().unwrap();
        assert!(help.contains(flag), "{flag} missing from help");
        assert_eq!(option["value_type"], "boolean");
        assert_eq!(option["required"], false);
        assert_eq!(option["min_values"], 0);
        assert_eq!(option["max_values"], 0);
        assert!(run(&[flag]).status.success(), "{flag} must be accepted");
    }
    let schema_flag = options
        .iter()
        .find(|option| option["name"] == "schema")
        .unwrap();
    assert_eq!(schema_flag["default"], serde_json::json!([false]));
    assert_eq!(schema["command"]["schema_conflicts_with_subcommands"], true);
}

#[test]
fn schema_examples_are_runnable() {
    let schema = json(&run(&["--schema"]));
    for example in schema["examples"].as_array().unwrap() {
        let args: Vec<_> = example
            .as_str()
            .unwrap()
            .split_whitespace()
            .skip(1)
            .collect();
        assert!(run(&args).status.success(), "{example} failed");
    }
}

#[test]
fn help_and_version_use_text_output() {
    for flag in ["--help", "-h", "--version", "-V"] {
        let output = run(&[flag]);
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert!(String::from_utf8_lossy(&output.stdout).contains("mcpstack"));
        assert!(serde_json::from_slice::<Value>(&output.stdout).is_err());
    }
}

#[test]
fn invalid_arguments_use_stderr_without_echoing_values() {
    for args in [
        vec!["--json"],
        vec!["--schema=true"],
        vec!["--schema", "--schema"],
        vec!["--schema", "--synthetic-secret=fixture-secret"],
        vec!["--synthetic-secret=fixture-secret", "--schema"],
        vec!["--", "--schema"],
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(diagnostic.contains("INVALID_ARGUMENT"));
        assert!(!diagnostic.contains("fixture-secret"));
    }
}

#[cfg(unix)]
#[test]
fn advertised_error_codes_match_process_failures() {
    let schema = json(&run(&["--schema"]));
    let statuses = schema["exit_statuses"].as_array().unwrap();
    let invalid = statuses
        .iter()
        .find(|entry| entry["error_code"] == "INVALID_ARGUMENT")
        .unwrap();
    let output = run(&["--unknown"]);
    assert_eq!(
        output.status.code().unwrap() as u64,
        invalid["status"].as_u64().unwrap()
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(invalid["error_code"].as_str().unwrap())
    );
    let output_error = statuses
        .iter()
        .find(|entry| entry["error_code"] == "OUTPUT_ERROR")
        .unwrap();
    // Close the peer before spawning so every write fails deterministically.
    let (reader, writer) = std::os::unix::net::UnixStream::pair().unwrap();
    drop(reader);
    let writer: std::os::fd::OwnedFd = writer.into();
    let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .arg("--schema")
        .env_remove("RUST_LOG")
        .env_remove("MCPSTACK_COLOR")
        .stdout(Stdio::from(writer))
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code().unwrap() as u64,
        output_error["status"].as_u64().unwrap()
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains(output_error["error_code"].as_str().unwrap())
    );
}

struct StackFixture {
    directory: tempfile::TempDir,
    file: std::path::PathBuf,
}

impl StackFixture {
    fn new(document: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("stack.yaml");
        std::fs::write(&file, document).unwrap();
        Self { directory, file }
    }

    fn validate(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_mcpstack"))
            .arg("validate")
            .arg(&self.file)
            .env_remove("MCPSTACK_MISSING_TEST_TOKEN")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
}

#[test]
fn validation_preserves_files_and_does_not_resolve_secrets_or_start_servers() {
    let document = r#"{
        "schema_version":1,
        "servers":{"local":{"transport":{
            "type":"stdio","command":"mcpstack-nonexistent-test-executable",
            "env":{"TOKEN":{"env":"MCPSTACK_MISSING_TEST_TOKEN"}}
        }}}
    }"#;
    let fixture = StackFixture::new(document);
    let output = fixture.validate();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Valid stack (schema 1, 1 servers).\n"
    );
    assert_eq!(std::fs::read_to_string(&fixture.file).unwrap(), document);
    assert_eq!(
        std::fs::read_dir(fixture.directory.path()).unwrap().count(),
        1
    );
}

#[test]
fn validation_reports_safe_typed_errors_and_missing_arguments() {
    for (document, expected) in [
        (
            r#"{"schema_version":2}"#,
            "Unsupported stack schema version",
        ),
        (
            r#"{"schema_version":1,"servers":"fixture-secret"}"#,
            "Malformed stack",
        ),
        (
            r#"{"schema_version":1,"servers":{" bad name":{"transport":{"type":"stdio","command":"example"}}}}"#,
            "Server names",
        ),
    ] {
        let fixture = StackFixture::new(document);
        let output = fixture.validate();
        assert_eq!(output.status.code(), Some(4));
        assert!(output.stdout.is_empty());
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(diagnostic.contains("INVALID_STACK:"));
        assert!(diagnostic.contains(expected));
        assert!(!diagnostic.contains("fixture-secret"));
        assert_eq!(std::fs::read_to_string(&fixture.file).unwrap(), document);
    }
    let fixture = StackFixture::new("{}");
    std::fs::remove_file(&fixture.file).unwrap();
    let output = fixture.validate();
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("STACK_READ_ERROR:"));
    for args in [vec!["validate"], vec!["--schema", "validate", "stack.json"]] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn native_validation_reports_adapter_boundary_without_executing_helpers() {
    let document = r#"{
        "schema_version":1,
        "servers":{"native":{
            "client":"claude_code",
            "config":{
                "type":"http", "url":"https://${HOST}/mcp",
                "headersHelper":"mcpstack-nonexistent-test-executable",
                "headers":{"Authorization":{"$env":"MCPSTACK_MISSING_TEST_TOKEN"}}
            }
        }}
    }"#;
    let fixture = StackFixture::new(document);
    let output = fixture.validate();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("require client adapter validation"));
    assert!(!text.contains("MCPSTACK_MISSING_TEST_TOKEN"));
    assert_eq!(std::fs::read_to_string(&fixture.file).unwrap(), document);
    assert_eq!(
        std::fs::read_dir(fixture.directory.path()).unwrap().count(),
        1
    );
}

#[test]
fn export_exposes_secrets_only_when_explicitly_requested() {
    let document = "[mcp_servers.example]\ncommand='example'\nargs=['--token','argument-secret']\nurl='https://example.com/mcp'\nhttp_headers={Authorization='header-secret'}\nenv={TOKEN='environment-secret'}\n";
    let fixture = StackFixture::new(document);
    let config = fixture.directory.path().join("config.toml");
    std::fs::write(&config, document).unwrap();
    for args in [
        vec!["export", "codex"],
        vec!["--non-interactive", "export", "codex"],
        vec!["export", "codex", "--non-interactive"],
        vec!["export", "codex", "--expose-secrets"],
        vec!["export", "--expose-secrets", "codex"],
        vec!["--non-interactive", "export", "codex", "--expose-secrets"],
    ] {
        let exposed = args.contains(&"--expose-secrets");
        let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
            .args(args)
            .arg("-vv")
            .env("PATH", "")
            .env("CODEX_HOME", fixture.directory.path())
            .env_remove("RUST_LOG")
            .env_remove("MCPSTACK_COLOR")
            .env_remove("NO_COLOR")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let yaml = String::from_utf8(output.stdout).unwrap();
        for secret in ["argument-secret", "header-secret", "environment-secret"] {
            assert_eq!(yaml.contains(secret), exposed);
            assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
        }
        let value: Value = yaml_serde::from_str(&yaml).unwrap();
        assert_eq!(value["servers"]["example"]["config"]["command"], "example");
        assert_eq!(
            value["servers"]["example"]["config"]["url"],
            "https://example.com/mcp"
        );
        std::fs::write(&fixture.file, yaml).unwrap();
        assert!(fixture.validate().status.success());
        assert_eq!(std::fs::read_to_string(&config).unwrap(), document);
    }
}

#[test]
fn codex_export_prints_a_stack_without_changing_values_or_files() {
    let document = "model='unrelated'\n[mcp_servers.local]\ncommand='example'\n[mcp_servers.local.env]\nTOKEN='fixture-secret'\n";
    let fixture = StackFixture::new(document);
    let config = fixture.directory.path().join("config.toml");
    std::fs::write(&config, document).unwrap();
    let execute = || {
        Command::new(env!("CARGO_BIN_EXE_mcpstack"))
            .args(["export", "codex"])
            .env("CODEX_HOME", fixture.directory.path())
            .env("PATH", "")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let first = execute();
    assert!(first.status.success());
    assert!(first.stderr.is_empty());
    assert!(String::from_utf8_lossy(&first.stdout).starts_with("schema_version: 1\n"));
    assert_eq!(first.stdout, execute().stdout);
    std::fs::write(&fixture.file, &first.stdout).unwrap();
    assert!(fixture.validate().status.success());
    let value: Value = yaml_serde::from_slice(&first.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(
        value["servers"]["local"]["config"]["env"]["TOKEN"],
        serde_json::json!({"$env":"MCPSTACK_LOCAL_ENV_TOKEN"})
    );
    assert_eq!(std::fs::read_to_string(&config).unwrap(), document);
    std::fs::write(&config, "invalid TOML fixture-secret").unwrap();
    let failed = execute();
    assert_eq!(failed.status.code(), Some(6));
    assert!(failed.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&failed.stderr).contains("fixture-secret"));
    // On Unix BaseDirs honors HOME; Windows uses the native known-folder API.
    #[cfg(unix)]
    {
        let home = fixture.directory.path().join("home");
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::write(home.join(".codex/config.toml"), document).unwrap();
        let fallback = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
            .args(["export", "codex"])
            .env_remove("CODEX_HOME")
            .env("HOME", home)
            .env("PATH", "")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(fallback.status.success());
        assert_eq!(fallback.stdout, first.stdout);
    }
}

#[test]
fn export_reads_config_without_finding_or_running_codex() {
    let fixture = StackFixture::new("unchanged");
    let root = fixture.directory.path();
    std::fs::write(
        root.join("config.toml"),
        "[mcp_servers.example]\ncommand='example'\n",
    )
    .unwrap();
    let marker = root.join("codex-was-run");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let executable = root.join("codex");
        std::fs::write(
            &executable,
            "#!/bin/sh\nprintf invoked > \"$MCPSTACK_TEST_MARKER\"\nexit 99\n",
        )
        .unwrap();
        std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[cfg(windows)]
    std::fs::write(
        root.join("codex.cmd"),
        "@echo off\r\necho invoked > \"%MCPSTACK_TEST_MARKER%\"\r\nexit /b 99\r\n",
    )
    .unwrap();
    for path in [root.to_path_buf(), root.join("missing-bin")] {
        let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
            .args(["--non-interactive", "export", "codex"])
            .env("CODEX_HOME", root)
            .env("PATH", path)
            .env("MCPSTACK_TEST_MARKER", &marker)
            .env_remove("RUST_LOG")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let stack: Value = yaml_serde::from_slice(&output.stdout).unwrap();
        assert_eq!(stack["servers"]["example"]["config"]["command"], "example");
        assert!(!marker.exists());
    }
}

#[test]
fn export_rejects_obsolete_and_unknown_fields_without_echoing_secrets() {
    for field in ["bearer_token='fixture-secret'", "future='fixture-secret'"] {
        let fixture = StackFixture::new("unchanged");
        let config = format!("[mcp_servers.remote]\nurl='https://example.com/mcp'\n{field}\n");
        std::fs::write(fixture.directory.path().join("config.toml"), &config).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
            .args(["export", "codex"])
            .env("CODEX_HOME", fixture.directory.path())
            .env("PATH", "")
            .env_remove("RUST_LOG")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(6));
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-secret"));
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("config.toml")).unwrap(),
            config
        );
    }
}

#[test]
fn logging_conflicts_are_rejected_across_subcommand_levels() {
    for args in [
        vec!["-v", "validate", "/missing", "-q"],
        vec!["-q", "validate", "/missing", "-v"],
        vec!["--log", "off", "validate", "/missing", "-v"],
        vec!["-v", "validate", "/missing", "--log", "off"],
        vec!["--log", "off", "validate", "/missing", "-q"],
        vec!["-q", "validate", "/missing", "--log", "off"],
        vec!["-v", "export", "codex", "-q"],
        vec!["export", "--log", "off", "codex", "-v"],
        vec!["-q", "export", "codex", "--log", "off"],
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert!(String::from_utf8_lossy(&output.stderr).starts_with("INVALID_ARGUMENT:"));
    }
}

#[test]
fn logging_filters_and_ansi_controls_keep_results_on_stdout() {
    let fixture = StackFixture::new("schema_version: 1\nservers: {}");
    let path = fixture.file.to_str().unwrap();
    for (color, ansi) in [("auto", false), ("never", false), ("always", true)] {
        let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
            .args(["--color", color, "validate", "/mcpstack-missing-test-stack"])
            .env_remove("RUST_LOG")
            .env_remove("NO_COLOR")
            .env_remove("MCPSTACK_COLOR")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(3));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr.contains(&0x1b), ansi);
        assert!(String::from_utf8_lossy(&output.stderr).contains("STACK_READ_ERROR"));
    }
    for args in [
        vec!["-v", "validate", path],
        vec!["validate", path, "-vv"],
        vec!["--quiet", "validate", path],
        vec!["--log", "off", "validate", path],
    ] {
        let output = run(&args);
        assert!(output.status.success(), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("Valid stack"));
        assert!(!output.stdout.contains(&0x1b));
    }
    let no_color = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args([
            "--color",
            "always",
            "validate",
            "/mcpstack-missing-test-stack",
        ])
        .env("NO_COLOR", "1")
        .env_remove("RUST_LOG")
        .output()
        .unwrap();
    assert!(!no_color.stderr.contains(&0x1b));
    let flag = run(&[
        "--color",
        "always",
        "--no-color",
        "validate",
        "/mcpstack-missing-test-stack",
    ]);
    assert_eq!(flag.status.code(), Some(3));
    assert!(!flag.stderr.contains(&0x1b));
    for args in [vec!["--log", "[", "validate", path], vec!["-v", "-q"]] {
        let output = run(&args);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    let schema = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .arg("--schema")
        .env("MCPSTACK_COLOR", "always")
        .env_remove("RUST_LOG")
        .output()
        .unwrap();
    assert!(schema.status.success());
    assert!(serde_json::from_slice::<Value>(&schema.stdout).is_ok());
    assert!(!schema.stdout.contains(&0x1b));
}

#[test]
fn export_logs_respect_verbosity_filters_and_color() {
    let fixture = StackFixture::new("unchanged");
    std::fs::write(
        fixture.directory.path().join("config.toml"),
        "[mcp_servers.example]\ncommand='example'\nenv={TOKEN='fixture-secret'}",
    )
    .unwrap();
    for (args, filter, debug, warning, ansi) in [
        (
            vec!["-v", "--color", "always", "export", "codex"],
            None,
            true,
            false,
            true,
        ),
        (
            vec!["export", "codex", "--quiet"],
            None,
            false,
            false,
            false,
        ),
        (
            vec!["--log", "error", "export", "codex"],
            None,
            false,
            false,
            false,
        ),
        (
            vec!["export", "codex"],
            Some("mcpstack=debug"),
            true,
            false,
            false,
        ),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mcpstack"));
        command
            .args(args)
            .env("PATH", "")
            .env("CODEX_HOME", fixture.directory.path())
            .env_remove("RUST_LOG")
            .env_remove("NO_COLOR")
            .env_remove("MCPSTACK_COLOR");
        if let Some(filter) = filter {
            command.env("RUST_LOG", filter);
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let value: Value = yaml_serde::from_slice(&output.stdout).unwrap();
        assert_eq!(
            value["servers"]["example"]["config"]["env"]["TOKEN"],
            serde_json::json!({"$env":"MCPSTACK_EXAMPLE_ENV_TOKEN"})
        );
        assert!(!output.stdout.contains(&0x1b));
        let log = String::from_utf8_lossy(&output.stderr);
        assert_eq!(log.contains("DEBUG"), debug);
        assert_eq!(log.contains("WARN"), warning);
        assert_eq!(output.stderr.contains(&0x1b), ansi);
        assert!(!log.contains("fixture-secret"));
    }
}

#[test]
fn export_reads_explicit_config_and_preserves_inputs() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let default = "[mcp_servers.default]\ncommand='default'\n";
    std::fs::write(root.join("config.toml"), default).unwrap();
    let selected = root.join("alternate config.toml");
    let valid = "[mcp_servers.selected]\ncommand='selected'\nenv={TOKEN='synthetic-secret'}\n";
    for (document, status) in [
        (Some(valid), 0),
        (Some("token='synthetic-secret"), 6),
        (None, 5),
    ] {
        if let Some(document) = document {
            std::fs::write(&selected, document).unwrap();
        } else {
            std::fs::remove_file(&selected).unwrap();
        }
        for path in [
            selected.clone(),
            std::path::PathBuf::from("alternate config.toml"),
        ] {
            let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
                .args(["--non-interactive", "export", "codex", "--config"])
                .arg(path)
                .current_dir(root)
                .env("CODEX_HOME", root)
                .env("PATH", "")
                .env_remove("RUST_LOG")
                .env_remove("MCPSTACK_COLOR")
                .env_remove("NO_COLOR")
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(status), "{output:?}");
            assert!(!String::from_utf8_lossy(&output.stderr).contains("synthetic-secret"));
            if status == 0 {
                let stack: Value = yaml_serde::from_slice(&output.stdout).unwrap();
                assert!(stack["servers"].get("selected").is_some());
                assert!(stack["servers"].get("default").is_none());
                assert!(!String::from_utf8_lossy(&output.stdout).contains("synthetic-secret"));
            } else {
                assert!(output.stdout.is_empty());
            }
        }
        assert_eq!(
            std::fs::read_to_string(root.join("config.toml")).unwrap(),
            default
        );
        if let Some(document) = document {
            assert_eq!(std::fs::read_to_string(&selected).unwrap(), document);
        }
    }
}

#[cfg(unix)]
#[test]
fn import_merges_resolved_stacks_with_private_backup_and_noop_repeat() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = StackFixture::new(
        "schema_version: 1\nservers:\n  shared:\n    client: codex\n    config:\n      command: tool\n      env: {API_KEY: {'$env': MCPSTACK_IMPORT_TEST_SECRET}}\n",
    );
    let config = fixture.directory.path().join("config.toml");
    let original = "# keep comment\nmodel='test'\n[mcp_servers.local]\ncommand='local'\n";
    std::fs::write(&config, original).unwrap();
    let execute = || {
        Command::new(env!("CARGO_BIN_EXE_mcpstack"))
            .args(["--non-interactive", "import", "codex", "-y"])
            .arg(&fixture.file)
            .arg("--config")
            .arg(&config)
            .env("PATH", "")
            .env("MCPSTACK_IMPORT_TEST_SECRET", "fixture-import-secret")
            .env_remove("RUST_LOG")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let output = execute();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("Imported 1"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("fixture-import-secret"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-import-secret"));
    let written = std::fs::read_to_string(&config).unwrap();
    assert!(written.starts_with(original));
    let parsed: toml::Table = toml::from_str(&written).unwrap();
    assert_eq!(
        parsed["mcp_servers"]["shared"]["env"]["API_KEY"].as_str(),
        Some("fixture-import-secret")
    );
    let backup = fixture.directory.path().join("config.toml.bak");
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
    assert_eq!(
        std::fs::metadata(&config).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let output = execute();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
    std::fs::rename(&backup, fixture.directory.path().join("prior-backup")).unwrap();
    let modified = std::fs::metadata(&config).unwrap().modified().unwrap();
    let output = execute();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("No changes"));
    assert_eq!(std::fs::read_to_string(&config).unwrap(), written);
    assert_eq!(
        std::fs::metadata(&config).unwrap().modified().unwrap(),
        modified
    );
}

#[cfg(unix)]
#[test]
fn import_validation_failures_leave_config_and_backup_unchanged() {
    let original = "[mcp_servers.shared]\ncommand='original'\n";
    for (document, status) in [
        ("broken: [fixture-secret", 4),
        (
            "schema_version: 1\nservers:\n  shared:\n    client: codex\n    config: {command: tool, args: [{'$env': MCPSTACK_MISSING_IMPORT_TOKEN}]}\n",
            10,
        ),
        (
            "schema_version: 1\nservers:\n  shared:\n    client: codex\n    config: {command: other}\n",
            12,
        ),
        (
            "schema_version: 1\nservers:\n  shared:\n    client: codex\n    config: {command: other, future: fixture-secret}\n",
            10,
        ),
    ] {
        let fixture = StackFixture::new(document);
        let config = fixture.directory.path().join("config.toml");
        std::fs::write(&config, original).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
            .args(["import", "codex", "--auto-approve"])
            .arg(&fixture.file)
            .env("PATH", "")
            .env("CODEX_HOME", fixture.directory.path())
            .env_remove("MCPSTACK_MISSING_IMPORT_TOKEN")
            .env_remove("RUST_LOG")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(status), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-secret"));
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
        assert!(!fixture.directory.path().join("config.toml.bak").exists());
    }
    let fixture = StackFixture::new(
        "schema_version: 1\nservers:\n  added:\n    client: codex\n    config: {command: tool}\n",
    );
    let config = fixture.directory.path().join("config.toml");
    let backup = fixture.directory.path().join("config.toml.bak");
    std::fs::write(&config, original).unwrap();
    std::fs::write(&backup, "keep backup").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args(["import", "codex", "--auto-approve"])
        .arg(&fixture.file)
        .arg("--config")
        .arg(&config)
        .env("PATH", "")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(11));
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), "keep backup");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
}

#[cfg(not(unix))]
#[test]
fn import_refuses_platforms_without_private_write_support() {
    let fixture = StackFixture::new(
        "schema_version: 1\nservers:\n  new:\n    client: codex\n    config: {command: tool}\n",
    );
    let config = fixture.directory.path().join("config.toml");
    std::fs::write(&config, "# original").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args(["import", "codex", "--auto-approve"])
        .arg(&fixture.file)
        .arg("--config")
        .arg(&config)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(13));
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "# original");
    assert!(!fixture.directory.path().join("config.toml.bak").exists());
}

#[cfg(unix)]
#[test]
fn codex_export_import_round_trip_preserves_supported_fields() {
    let fixture = StackFixture::new("placeholder");
    let root = fixture.directory.path();
    let source = root.join("source.toml");
    let target = root.join("target.toml");
    let original = "# source settings are not shared\nmodel='private'\n[mcp_servers.process]\ncommand='tool'\nargs=['--token=fixture-roundtrip-secret']\ncwd='/tmp'\nenv_vars=['FORWARDED']\nenabled=false\nrequired=true\nstartup_timeout_sec=10\ntool_timeout_sec=20\nenabled_tools=['read']\ndisabled_tools=['write']\n[mcp_servers.process.env]\nAPI_KEY='fixture-roundtrip-secret'\n[mcp_servers.remote]\nurl='https://example.com/mcp'\nbearer_token_env_var='CODEX_TOKEN'\n[mcp_servers.remote.http_headers]\nAuthorization='fixture-roundtrip-secret'\n[mcp_servers.remote.env_http_headers]\nX-Auth='OTHER_TOKEN'\n";
    std::fs::write(&source, original).unwrap();
    let exported = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args(["--non-interactive", "export", "codex", "--config"])
        .arg(&source)
        .env("PATH", "")
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(exported.status.success(), "{exported:?}");
    assert!(!String::from_utf8_lossy(&exported.stdout).contains("fixture-roundtrip-secret"));
    std::fs::write(&fixture.file, &exported.stdout).unwrap();
    let stack: serde_json::Value = yaml_serde::from_slice(&exported.stdout).unwrap();
    let config = &stack["servers"]["process"]["config"];
    let argument = config["args"][0]["$env"].as_str().unwrap();
    let api_key = config["env"]["API_KEY"]["$env"].as_str().unwrap();
    let authorization =
        stack["servers"]["remote"]["config"]["http_headers"]["Authorization"]["$env"]
            .as_str()
            .unwrap();
    let imported = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args(["import", "codex", "--auto-approve"])
        .arg(&fixture.file)
        .arg("--config")
        .arg(&target)
        .env("PATH", "")
        .env_remove("RUST_LOG")
        .env(argument, "--token=fixture-roundtrip-secret")
        .env(api_key, "fixture-roundtrip-secret")
        .env(authorization, "fixture-roundtrip-secret")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(imported.status.success(), "{imported:?}");
    let before: toml::Table = toml::from_str(original).unwrap();
    let after: toml::Table = toml::from_str(&std::fs::read_to_string(&target).unwrap()).unwrap();
    for (name, definition) in before["mcp_servers"].as_table().unwrap() {
        for (field, expected) in definition.as_table().unwrap() {
            let actual = &after["mcp_servers"][name][field];
            assert_eq!(actual, expected, "{name}.{field}");
        }
    }
    assert!(!after.contains_key("model"));
    assert_eq!(std::fs::read(root.join("target.toml.bak")).unwrap(), b"");
    assert_eq!(std::fs::read_to_string(&source).unwrap(), original);
}

#[test]
fn import_dry_run_is_read_only_redacted_and_available_without_a_terminal() {
    let fixture = StackFixture::new(
        "schema_version: 1\nservers:\n  added:\n    client: codex\n    config:\n      command: tool\n      args: [{'$env': MCPSTACK_PREVIEW_ARGUMENT}]\n      env: {API_KEY: literal-preview-secret}\n",
    );
    let config = fixture.directory.path().join("config.toml");
    let backup = fixture.directory.path().join("config.toml.bak");
    let original =
        "# preserve\nmodel='private-model-value'\n[mcp_servers.existing]\ncommand='existing'\n";
    std::fs::write(&config, original).unwrap();
    std::fs::write(&backup, "existing backup").unwrap();
    let modified = std::fs::metadata(&config).unwrap().modified().unwrap();
    for target_exists in [true, false] {
        if !target_exists {
            std::fs::remove_file(&config).unwrap();
        }
        let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
            .args(["--non-interactive", "import", "--dry-run", "codex"])
            .arg(&fixture.file)
            .arg("--config")
            .arg(&config)
            .arg("-y") // dry-run takes precedence over approval
            .env("MCPSTACK_PREVIEW_ARGUMENT", "arbitrary-preview-secret")
            .env_remove("RUST_LOG")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let diff = String::from_utf8(output.stdout).unwrap();
        assert!(diff.contains("--- /dev/null\n+++ "));
        assert!(diff.contains("@@ -0,0 +1,"));
        assert!(diff.contains("+[mcp_servers.added]"));
        assert!(diff.contains("+command = \"tool\""));
        assert!(diff.contains("<redacted>"));
        for secret in [
            "literal-preview-secret",
            "arbitrary-preview-secret",
            "private-model-value",
        ] {
            assert!(!diff.contains(secret), "{diff}");
            assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
        }
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "existing backup");
        if target_exists {
            assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
            assert_eq!(
                std::fs::metadata(&config).unwrap().modified().unwrap(),
                modified
            );
        } else {
            assert!(!config.exists());
        }
    }
    std::fs::remove_file(&backup).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args(["import", "codex", "--dry-run"])
        .arg(&fixture.file)
        .arg("--config")
        .arg(&config)
        .env("MCPSTACK_PREVIEW_ARGUMENT", "arbitrary-preview-secret")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(!config.exists());
    assert!(!backup.exists());
}

#[test]
fn import_requires_explicit_approval_without_a_terminal() {
    let fixture = StackFixture::new(
        "schema_version: 1\nservers:\n  new:\n    client: codex\n    config: {command: tool}\n",
    );
    let config = fixture.directory.path().join("config.toml");
    for non_interactive in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mcpstack"));
        if non_interactive {
            command.arg("--non-interactive");
        }
        let output = command
            .args(["import", "codex"])
            .arg(&fixture.file)
            .arg("--config")
            .arg(&config)
            .env_remove("RUST_LOG")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(10), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("--auto-approve"));
        assert!(!config.exists());
        assert!(!fixture.directory.path().join("config.toml.bak").exists());
    }
}
