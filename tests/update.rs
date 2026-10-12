use std::process::{Command, Output, Stdio};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args(args)
        .env_remove("RUST_LOG")
        .env_remove("MCPSTACK_COLOR")
        .env_remove("MCPSTACK_SKIP_VERSION_CHECK")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn update_help_and_schema_advertise_approval_check_and_opt_out() {
    let output = run(&["update", "--help"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).unwrap();
    for text in [
        "--check",
        "--auto-approve",
        "--skip-version-check",
        "MCPSTACK_SKIP_VERSION_CHECK",
    ] {
        assert!(help.contains(text), "{help}");
    }
    let output = run(&["--schema"]);
    let schema: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let update = schema["command"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|command| command["name"] == "update")
        .unwrap();
    let check = update["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["long"] == "--check")
        .unwrap();
    assert!(
        check["conflicts_with"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name == "auto_approve")
    );
    for (name, status) in [("UPDATE_CHECK_ERROR", 15), ("UPDATE_ERROR", 16)] {
        assert!(
            schema["exit_statuses"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["error_code"] == name && entry["status"] == status)
        );
    }
}

#[test]
fn unattended_update_requires_explicit_approval_without_network_or_writes() {
    for args in [
        vec!["update"],
        vec!["--non-interactive", "update"],
        vec!["update", "--non-interactive"],
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("mcpstack update --non-interactive -y")
        );
    }
}

#[test]
fn development_binary_refuses_self_modification_even_with_approval() {
    let output = run(&["update", "--non-interactive", "-y"]);
    assert_eq!(output.status.code(), Some(16), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not a tracked Cargo installation"));
}

#[test]
fn invalid_update_arguments_are_rejected_without_network() {
    for args in [
        vec!["update", "--check", "-y"],
        vec!["--schema", "update"],
        vec!["update", "unexpected-secret"],
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("unexpected-secret"));
    }
}

#[test]
fn opt_out_works_at_each_cli_level_and_keeps_machine_output_clean() {
    for args in [
        vec!["--skip-version-check", "--schema"],
        vec!["codex", "--skip-version-check", "show", "--help"],
        vec!["codex", "show", "--skip-version-check", "--help"],
    ] {
        let output = run(&args);
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
    }
    let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args(["--schema"])
        .env("MCPSTACK_SKIP_VERSION_CHECK", "true")
        .env_remove("RUST_LOG")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap();
}
