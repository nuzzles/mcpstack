use std::process::{Command, Output, Stdio};

use serde_json::Value;

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args(args)
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
    assert_eq!(schema["command"]["commands"], serde_json::json!([]));

    let help = run(&["--help"]);
    let help = String::from_utf8(help.stdout).unwrap();
    let options = schema["command"]["options"].as_array().unwrap();
    assert_eq!(options.len(), 5);
    for option in options {
        let flag = option["long"].as_str().unwrap();
        assert!(help.contains(flag), "{flag} missing from help");
        assert_eq!(option["value_type"], "boolean");
        assert_eq!(option["required"], false);
        assert_eq!(option["min_values"], 0);
        assert_eq!(option["max_values"], 0);
        assert!(run(&[flag]).status.success(), "{flag} must be accepted");
    }
    let json_flag = options
        .iter()
        .find(|option| option["name"] == "json")
        .unwrap();
    assert_eq!(json_flag["default"], serde_json::json!([false]));
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
fn json_results_and_noninteractive_mode_work_without_stdin() {
    let output = run(&["--json", "--non-interactive"]);
    assert!(output.status.success());
    let result = json(&output);
    assert_eq!(result["ok"], true);
    assert_eq!(result["result"]["cli_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(result["result"]["status"], "in_development");
}

#[test]
fn help_and_version_respect_json_mode_and_short_flags() {
    for (flag, kind) in [
        ("--help", "help"),
        ("-h", "help"),
        ("--version", "version"),
        ("-V", "version"),
    ] {
        let output = run(&[flag, "--json"]);
        assert!(output.status.success());
        let result = json(&output);
        assert_eq!(result["ok"], true);
        assert_eq!(result["result"]["kind"], kind);
        assert!(
            result["result"]["text"]
                .as_str()
                .unwrap()
                .contains("mcpstack")
        );
    }
}

#[test]
fn parse_errors_are_structured_without_echoing_argument_values() {
    for args in [
        vec!["--json", "--synthetic-secret=fixture-secret"],
        vec!["--synthetic-secret=fixture-secret", "--json"],
        vec!["--schema", "--synthetic-secret=fixture-secret"],
        vec!["--json", "--json"],
        vec!["--json", "--non-interactive=true"],
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(2));
        let result = json(&output);
        assert_eq!(result["ok"], false);
        assert_eq!(result["error"]["code"], "INVALID_ARGUMENT");
        assert!(
            result["error"]["message"]
                .as_str()
                .unwrap()
                .contains("--schema")
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("fixture-secret"));
    }
}

#[test]
fn human_parse_errors_use_stderr_and_end_of_options_is_respected() {
    for args in [
        vec!["--unknown"],
        vec!["--", "--json"],
        vec!["--", "--schema"],
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}
