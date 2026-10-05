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
    assert_eq!(commands.len(), 3); // validate, export, and generated help
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
    assert_eq!(options.len(), 8);
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
        vec!["--non-interactive"],
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

#[cfg(unix)]
#[test]
fn codex_export_prints_a_stack_without_changing_values_or_files() {
    let document = "model='unrelated'\n[mcp_servers.local]\ncommand='example'\n[mcp_servers.local.env]\nTOKEN='fixture-secret'\n";
    let fixture = StackFixture::new(document);
    let config = fixture.directory.path().join("config.toml");
    let bin = mock_codex(fixture.directory.path(), "codex-cli 0.149.0", 0);
    std::fs::write(&config, document).unwrap();
    let execute = || {
        Command::new(env!("CARGO_BIN_EXE_mcpstack"))
            .args(["export", "codex"])
            .env("CODEX_HOME", fixture.directory.path())
            .env("PATH", &bin)
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
        "fixture-secret"
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
            .env("PATH", &bin)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(fallback.status.success());
        assert_eq!(fallback.stdout, first.stdout);
    }
}

#[cfg(unix)]
fn mock_codex(root: &std::path::Path, version: &str, status: u8) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let executable = bin.join("codex");
    std::fs::write(&executable, format!("#!/bin/sh\n[ \"$1\" = \"--version\" ] || exit 99\nprintf '%s\\n' '{version}'\nexit {status}\n")).unwrap();
    std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    bin
}

#[cfg(unix)]
#[test]
fn export_rejects_detection_failures_and_major_versions_before_reading_config() {
    for (version, status, expected) in [
        ("codex-cli 1.0.0", 0, 8),
        ("codex-cli 1.0.0-alpha.1", 0, 8),
        ("codex-cli 2.0.0", 0, 8),
        ("fixture-secret", 0, 7),
        ("codex-cli 0.149.0", 1, 7),
    ] {
        let fixture = StackFixture::new("unchanged");
        let bin = mock_codex(fixture.directory.path(), version, status);
        // No config.toml: version failure must precede config discovery/reads.
        let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
            .args(["export", "codex"])
            .env("PATH", bin)
            .env("CODEX_HOME", fixture.directory.path())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(expected));
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-secret"));
        assert_eq!(std::fs::read_to_string(&fixture.file).unwrap(), "unchanged");
        assert!(!fixture.directory.path().join("config.toml.bak").exists());
    }
    let fixture = StackFixture::new("unchanged");
    let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args(["export", "codex"])
        .env("PATH", fixture.directory.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert!(output.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn older_versions_export_and_newer_versions_warn_only_on_stderr() {
    for version in [
        "0.0.0",
        "0.50.0",
        "0.149.1",
        "0.160.0",
        "0.160.1",
        "0.161.0-alpha.1",
    ] {
        let fixture = StackFixture::new("unchanged");
        let config = "[mcp_servers.example]\ncommand='example'\nstartup_timeout_ms=1000\n";
        std::fs::write(fixture.directory.path().join("config.toml"), config).unwrap();
        let bin = mock_codex(fixture.directory.path(), &format!("codex-cli {version}"), 0);
        let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
            .args(["export", "codex"])
            .env("PATH", bin)
            .env("CODEX_HOME", fixture.directory.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        let stack: Value = yaml_serde::from_slice(&output.stdout).unwrap();
        assert_eq!(
            stack["servers"]["example"]["config"]["startup_timeout_ms"],
            1000
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("WARN"));
        let newer = matches!(version, "0.160.1" | "0.161.0-alpha.1");
        assert_eq!(!output.stderr.is_empty(), newer);
        if newer {
            assert!(String::from_utf8_lossy(&output.stderr).contains("WARN"));
        }
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("config.toml")).unwrap(),
            config
        );
    }
}

#[cfg(windows)]
#[test]
fn windows_npm_launcher_exports_and_reports_detection_failures() {
    let fixture = StackFixture::new("unchanged");
    let bin = fixture.directory.path().join("npm tools & spaces");
    std::fs::create_dir_all(&bin).unwrap();
    let launcher = bin.join("codex.cmd");
    let config = "[mcp_servers.example]\ncommand='example'\n";
    std::fs::write(fixture.directory.path().join("config.toml"), config).unwrap();
    let execute = || {
        Command::new(env!("CARGO_BIN_EXE_mcpstack"))
            .args(["export", "codex"])
            .env("PATH", &bin)
            .env("CODEX_HOME", fixture.directory.path())
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    std::fs::write(
        &launcher,
        "@echo off\r\nif not \"%~1\"==\"--version\" exit /b 99\r\necho codex-cli 0.149.0\r\n",
    )
    .unwrap();
    let output = execute();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let value: Value = yaml_serde::from_slice(&output.stdout).unwrap();
    assert_eq!(value["servers"]["example"]["config"]["command"], "example");
    for version in ["1.0.0", "1.0.0-alpha.1"] {
        std::fs::write(
            &launcher,
            format!("@echo off\r\necho codex-cli {version}\r\n"),
        )
        .unwrap();
        let rejected = execute();
        assert_eq!(rejected.status.code(), Some(8));
        assert!(rejected.stdout.is_empty());
        assert!(String::from_utf8_lossy(&rejected.stderr).contains("UNSUPPORTED_CLIENT_VERSION"));
    }
    std::fs::write(
        &launcher,
        "@echo off\r\necho codex-cli 0.149.0\r\nexit /b 1\r\n",
    )
    .unwrap();
    let failed = execute();
    assert_eq!(failed.status.code(), Some(7));
    assert!(failed.stdout.is_empty());
    assert_eq!(
        std::fs::read_to_string(fixture.directory.path().join("config.toml")).unwrap(),
        config
    );
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

#[cfg(unix)]
#[test]
fn export_logs_respect_verbosity_filters_and_color() {
    let fixture = StackFixture::new("unchanged");
    let bin = mock_codex(fixture.directory.path(), "codex-cli 0.161.0", 0);
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
            true,
            true,
        ),
        (vec!["export", "codex", "--quiet"], None, false, true, false),
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
            true,
            false,
        ),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mcpstack"));
        command
            .args(args)
            .env("PATH", &bin)
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
            "fixture-secret"
        );
        assert!(!output.stdout.contains(&0x1b));
        let log = String::from_utf8_lossy(&output.stderr);
        assert_eq!(log.contains("DEBUG"), debug);
        assert_eq!(log.contains("WARN"), warning);
        assert_eq!(output.stderr.contains(&0x1b), ansi);
        assert!(!log.contains("fixture-secret"));
    }
}
