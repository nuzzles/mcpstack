use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args(args)
        .env_remove("RUST_LOG")
        .env_remove("MCPSTACK_COLOR")
        .env_remove("NO_COLOR")
        .env_remove("MCPSTACK_JSON_MISSING")
        .env("MCPSTACK_JSON_SECRET", "new-fixture-credential")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn report(output: &Output) -> Value {
    let result: Value = serde_json::from_slice(&output.stdout).expect("exactly one JSON document");
    assert_eq!(result["schema_version"], 1);
    assert!(!output.stdout.contains(&0x1b));
    if output.status.success() {
        assert_eq!(result["status"], "success");
        assert_eq!(result["error"], Value::Null);
    } else {
        assert_eq!(result["status"], "failure");
        assert_eq!(
            result["error"]["exit_status"],
            output.status.code().unwrap()
        );
    }
    for secret in [
        "new-fixture-credential",
        "old-fixture-credential",
        "malformed-fixture-secret",
    ] {
        assert!(!String::from_utf8_lossy(&output.stdout).contains(secret));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
    }
    result
}

fn fixture(root: &Path) -> (String, String, String) {
    let config = root.join("config.toml");
    let stack = root.join("stack.yml");
    let original = "model='private-fixture-model'\n[mcp_servers.replace]\ncommand='tool'\nargs=['old-fixture-credential']\n[mcp_servers.same]\ncommand='same-tool'\n";
    std::fs::write(&config, original).unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers:\n  added:\n    client: codex\n    config: {command: added-tool}\n  replace:\n    client: codex\n    config: {command: tool, args: [{'$env': MCPSTACK_JSON_SECRET}]}\n  same:\n    client: codex\n    config: {command: same-tool}\n").unwrap();
    (
        stack.to_str().unwrap().into(),
        config.to_str().unwrap().into(),
        original.into(),
    )
}

#[test]
fn validation_json_is_deterministic_and_never_contains_definitions() {
    let directory = tempfile::tempdir().unwrap();
    let (stack, _, _) = fixture(directory.path());
    let first = run(&["--json", "validate", &stack]);
    let second = run(&["validate", &stack, "--json"]);
    assert!(first.status.success());
    assert!(first.stderr.is_empty());
    assert_eq!(first.stdout, second.stdout);
    let value = report(&first);
    assert_eq!(value["operation"], "validate");
    assert_eq!(value["stack_schema_version"], 1);
    assert_eq!(value["integration"], Value::Null);
    assert_eq!(
        value["servers"],
        json!([
            {"name":"added", "action":"validate", "outcome":"validated"},
            {"name":"replace", "action":"validate", "outcome":"validated"},
            {"name":"same", "action":"validate", "outcome":"validated"},
        ])
    );
    assert!(!String::from_utf8_lossy(&first.stdout).contains("added-tool"));
}

#[test]
fn argument_logging_and_stack_failures_have_safe_json_errors() {
    for (args, code, status) in [
        (vec!["--json"], "INVALID_ARGUMENT", 2),
        (vec!["--json", "--schema"], "INVALID_ARGUMENT", 2),
        (vec!["export", "codex", "--json"], "INVALID_ARGUMENT", 2),
        (vec!["validate", "--json"], "INVALID_ARGUMENT", 2),
        (
            vec!["--json", "--unknown=malformed-fixture-secret"],
            "INVALID_ARGUMENT",
            2,
        ),
        (
            vec!["--json", "--log", "[", "validate", "missing.yml"],
            "LOGGING_ERROR",
            9,
        ),
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(status));
        assert_eq!(report(&output)["error"]["code"], code);
    }
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("invalid.yml");
    std::fs::write(&file, "malformed-fixture-secret: [").unwrap();
    let output = run(&["validate", file.to_str().unwrap(), "--json"]);
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(report(&output)["error"]["code"], "INVALID_STACK");
    std::fs::remove_file(&file).unwrap();
    let output = run(&["validate", file.to_str().unwrap(), "--json"]);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(report(&output)["error"]["code"], "STACK_READ_ERROR");
    // A flag after the argument terminator is a filename, not an output mode.
    let output = run(&["validate", "--", "--json"]);
    assert!(output.stdout.is_empty());
}

#[test]
fn diff_and_dry_run_json_report_actions_without_writes_or_ansi() {
    let directory = tempfile::tempdir().unwrap();
    let (stack, config, original) = fixture(directory.path());
    for command in [
        vec!["diff", "codex"],
        vec!["import", "codex", "--dry-run", "-y"],
    ] {
        let mut args = command.clone();
        args.extend([&stack, "--config", &config, "--json", "--color", "always"]);
        let output = run(&args);
        assert!(output.status.success(), "{output:?}");
        let value = report(&output);
        let dry_run = command[0] == "import";
        assert_eq!(value["operation"], command[0]);
        assert_eq!(value["integration"], "codex");
        assert_eq!(value["config_path"], config);
        assert_eq!(value["dry_run"], dry_run);
        assert_eq!(value["backup"]["status"], "not_requested");
        let changed_outcome = if dry_run { "approved" } else { "proposed" };
        assert_eq!(
            value["servers"],
            json!([
                {"name":"added", "action":"add", "outcome":changed_outcome},
                {"name":"replace", "action":"replace", "outcome":changed_outcome},
                {"name":"same", "action":"unchanged", "outcome":"unchanged"},
            ])
        );
        let diff = value["diff"].as_str().unwrap();
        assert!(diff.contains("<redacted: changed>"));
        assert!(!diff.contains('\x1b'));
        assert!(!diff.contains("private-fixture-model"));
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
    }
}

#[cfg(unix)]
#[test]
fn import_json_reports_applied_changes_backups_and_identical_repeats() {
    let directory = tempfile::tempdir().unwrap();
    let (stack, config, original) = fixture(directory.path());
    let args = [
        "import", "codex", &stack, "--config", &config, "-y", "--json",
    ];
    let output = run(&args);
    assert!(output.status.success(), "{output:?}");
    let value = report(&output);
    assert_eq!(value["backup"]["status"], "created");
    assert_eq!(value["backup"]["path"], format!("{config}.~1~"));
    assert_eq!(value["servers"][0]["outcome"], "applied");
    assert_eq!(value["servers"][1]["outcome"], "applied");
    assert_eq!(value["servers"][2]["outcome"], "unchanged");
    assert_eq!(value["diff"], Value::Null);
    assert_eq!(
        std::fs::read_to_string(format!("{config}.~1~")).unwrap(),
        original
    );
    let written = std::fs::read(&config).unwrap();
    assert!(String::from_utf8_lossy(&written).contains("new-fixture-credential"));
    let modified = std::fs::metadata(&config).unwrap().modified().unwrap();
    let output = run(&args);
    assert!(output.status.success());
    let value = report(&output);
    assert!(
        value["servers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|server| server["outcome"] == "unchanged")
    );
    assert_eq!(value["backup"]["path"], format!("{config}.~2~"));
    assert_eq!(std::fs::read(&config).unwrap(), written);
    assert_eq!(
        std::fs::metadata(&config).unwrap().modified().unwrap(),
        modified
    );
}

#[cfg(unix)]
#[test]
fn import_failure_json_retains_backup_and_never_claims_application() {
    let directory = tempfile::tempdir().unwrap();
    let (stack, config, original) = fixture(directory.path());
    for (document, code, status) in [
        ("malformed-fixture-secret: [", "INVALID_STACK", 4),
        (
            "schema_version: 1\nservers:\n  missing:\n    client: codex\n    config: {command: tool, args: [{'$env': MCPSTACK_JSON_MISSING}]}\n",
            "IMPORT_ERROR",
            10,
        ),
    ] {
        std::fs::write(&stack, document).unwrap();
        let output = run(&[
            "import", "codex", &stack, "--config", &config, "-y", "--json",
        ]);
        assert_eq!(output.status.code(), Some(status));
        let value = report(&output);
        assert_eq!(value["error"]["code"], code);
        assert_eq!(value["backup"]["status"], "created");
        assert_eq!(
            std::fs::read_to_string(value["backup"]["path"].as_str().unwrap()).unwrap(),
            original
        );
        assert!(
            value["servers"]
                .as_array()
                .unwrap()
                .iter()
                .all(|server| server["outcome"] != "applied")
        );
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    }
}

#[test]
fn json_does_not_implicitly_approve_imports() {
    let directory = tempfile::tempdir().unwrap();
    let (stack, config, original) = fixture(directory.path());
    for flags in [vec![], vec!["--dry-run"]] {
        let mut args = vec![
            "import",
            "codex",
            &stack,
            "--config",
            &config,
            "--json",
            "--non-interactive",
        ];
        args.extend(flags);
        let output = run(&args);
        assert_eq!(output.status.code(), Some(10));
        assert_eq!(report(&output)["error"]["code"], "IMPORT_ERROR");
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
    }
}

#[cfg(unix)]
#[test]
fn backup_discovery_failure_is_reported_without_modifying_files() {
    let directory = tempfile::tempdir().unwrap();
    let (stack, config, original) = fixture(directory.path());
    std::fs::write(format!("{config}.~18446744073709551616~"), "occupied").unwrap();
    let output = run(&[
        "import", "codex", &stack, "--config", &config, "-y", "--json",
    ]);
    assert_eq!(output.status.code(), Some(11));
    let value = report(&output);
    assert_eq!(value["backup"]["status"], "failed");
    assert_eq!(value["backup"]["path"], Value::Null);
    assert_eq!(value["error"]["code"], "BACKUP_ERROR");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 3);
}

#[cfg(not(unix))]
#[test]
fn unsupported_private_writes_return_a_structured_failure() {
    let directory = tempfile::tempdir().unwrap();
    let (stack, config, original) = fixture(directory.path());
    let output = run(&[
        "import", "codex", &stack, "--config", &config, "-y", "--json",
    ]);
    assert_eq!(output.status.code(), Some(13));
    let value = report(&output);
    assert_eq!(value["error"]["code"], "CONFIG_WRITE_ERROR");
    assert_eq!(value["backup"]["status"], "not_requested");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
}

#[test]
fn identical_previews_have_empty_diffs_and_config_failures_are_structured() {
    let directory = tempfile::tempdir().unwrap();
    let (stack, config, _) = fixture(directory.path());
    std::fs::write(&stack, "schema_version: 1\nservers:\n  same:\n    client: codex\n    config: {command: same-tool}\n").unwrap();
    for flags in [
        vec!["diff", "codex"],
        vec!["import", "codex", "--dry-run", "-y"],
    ] {
        let mut args = flags;
        args.extend([&stack, "--config", &config, "--json"]);
        let output = run(&args);
        assert!(output.status.success());
        let value = report(&output);
        assert_eq!(value["diff"], "");
        assert_eq!(value["servers"][0]["outcome"], "unchanged");
    }
    std::fs::write(&config, "malformed-fixture-secret: [").unwrap();
    let output = run(&["diff", "codex", &stack, "--config", &config, "--json"]);
    assert_eq!(output.status.code(), Some(13));
    let value = report(&output);
    assert_eq!(value["error"]["code"], "CONFIG_WRITE_ERROR");
    assert_eq!(value["diff"], Value::Null);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
}

#[cfg(unix)]
#[test]
fn json_stdout_failure_preserves_the_output_error_exit_status() {
    let directory = tempfile::tempdir().unwrap();
    let (stack, _, _) = fixture(directory.path());
    let (reader, writer) = std::os::unix::net::UnixStream::pair().unwrap();
    drop(reader);
    let writer: std::os::fd::OwnedFd = writer.into();
    let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args(["validate", &stack, "--json"])
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .stdout(Stdio::from(writer))
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("OUTPUT_ERROR"));
    assert!(output.stdout.is_empty());
}
