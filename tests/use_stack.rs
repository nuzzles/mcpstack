use std::path::Path;
use std::process::{Command, Output, Stdio};

fn execute(stack: &Path, config: &Path, flags: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args(["--non-interactive", "--color", "never", "use"])
        .arg(stack)
        .arg("--config")
        .arg(config)
        .args(flags)
        .env_remove("RUST_LOG")
        .env_remove("MCPSTACK_USE_TOKEN")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn servers(config: &Path) -> toml::Table {
    let document: toml::Table = toml::from_str(&std::fs::read_to_string(config).unwrap()).unwrap();
    document
        .get("mcp_servers")
        .and_then(toml::Value::as_table)
        .cloned()
        .unwrap_or_default()
}

#[test]
#[cfg(any(unix, windows))]
fn switches_between_files_replaces_all_servers_and_preserves_other_settings() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let work = dir.path().join("work.yml");
    let personal = dir.path().join("personal.yml");
    let original = "# preserve settings\nmodel = 'test' # keep\n[mcp_servers.unmanaged]\ncommand='remove-me'\n[mcp_servers.shared]\ncommand='old'\n[projects.fixture]\ntrust_level='trusted'\n";
    std::fs::write(&config, original).unwrap();
    std::fs::write(&work, "schema_version: 1\nservers:\n  shared:\n    config: {command: work}\n  work-only:\n    transport: {type: stdio, command: work-tool}\n").unwrap();
    std::fs::write(
        &personal,
        "schema_version: 1\nservers:\n  personal:\n    config: {url: 'https://example.com/mcp'}\n",
    )
    .unwrap();
    for (file, names) in [
        (&work, vec!["shared", "work-only"]),
        (&personal, vec!["personal"]),
        (&work, vec!["shared", "work-only"]),
    ] {
        let output = execute(file, &config, &["-y"]);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            servers(&config)
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            names
        );
        let text = std::fs::read_to_string(&config).unwrap();
        assert!(text.starts_with("# preserve settings\nmodel = 'test' # keep\n"));
        let document: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(
            document["projects"]["fixture"]["trust_level"].as_str(),
            Some("trusted")
        );
    }
    assert_eq!(
        std::fs::read_to_string(dir.path().join("config.toml.~1~")).unwrap(),
        original
    );
    assert_eq!(servers(&config)["shared"]["command"].as_str(), Some("work"));
    let contents = std::fs::read(&config).unwrap();
    let modified = std::fs::metadata(&config).unwrap().modified().unwrap();
    assert!(execute(&work, &config, &["-y"]).status.success());
    assert_eq!(std::fs::read(&config).unwrap(), contents);
    assert_eq!(
        std::fs::metadata(&config).unwrap().modified().unwrap(),
        modified
    );
    assert!(!dir.path().join("config.toml.~4~").exists());
}

#[test]
fn dry_run_shows_redacted_removals_and_changes_without_any_writes() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let stack = dir.path().join("stack.yml");
    let original = "model='private-model'\n[mcp_servers.removed]\ncommand='old'\nenv={API_KEY='fixture-removal-secret'}\n[mcp_servers.shared]\ncommand='before'\nenv={TOKEN='fixture-old-secret'}\n";
    std::fs::write(&config, original).unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers:\n  shared:\n    config: {command: after, env: {TOKEN: fixture-new-secret}}\n  added:\n    config: {command: added}\n").unwrap();
    let first = execute(&stack, &config, &["--dry-run"]);
    assert!(first.status.success(), "{first:?}");
    assert_eq!(
        first.stdout,
        execute(&stack, &config, &["--dry-run"]).stdout
    );
    let diff = String::from_utf8(first.stdout).unwrap();
    assert!(diff.contains("-[mcp_servers.removed]"), "{diff}");
    assert!(diff.contains("+[mcp_servers.added]"), "{diff}");
    assert!(diff.contains("+command = \"after\""), "{diff}");
    assert!(diff.contains("<redacted>"), "{diff}");
    for secret in [
        "fixture-removal-secret",
        "fixture-old-secret",
        "fixture-new-secret",
        "private-model",
    ] {
        assert!(!diff.contains(secret), "{diff}");
        assert!(!String::from_utf8_lossy(&first.stderr).contains(secret));
    }
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    // A removal-only switch must still be a real change, including to an empty stack.
    std::fs::write(&stack, "schema_version: 1\nservers: {}\n").unwrap();
    let output = execute(&stack, &config, &["--dry-run"]);
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("-[mcp_servers.removed]"));
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
}

#[test]
#[cfg(any(unix, windows))]
fn empty_stack_clears_inline_servers_and_absent_config_can_be_created() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let stack = dir.path().join("stack.yml");
    std::fs::write(
        &config,
        "model='preserved'\nmcp_servers={old={command='old'}}\n",
    )
    .unwrap();
    std::fs::write(&stack, "schema_version: 1\nservers: {}\n").unwrap();
    let output = execute(&stack, &config, &["-y"]);
    assert!(output.status.success(), "{output:?}");
    assert!(servers(&config).is_empty());
    assert!(
        std::fs::read_to_string(&config)
            .unwrap()
            .contains("model='preserved'")
    );
    std::fs::remove_file(&config).unwrap();
    let output = execute(&stack, &config, &["--dry-run"]);
    assert!(output.status.success(), "{output:?}");
    assert!(!config.exists());
    std::fs::write(
        &stack,
        "schema_version: 1\nservers:\n  new:\n    config: {command: new}\n",
    )
    .unwrap();
    assert!(execute(&stack, &config, &["--dry-run"]).status.success());
    assert!(!config.exists());
    let output = execute(&stack, &config, &["-y"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(servers(&config).len(), 1);
}

#[test]
fn rejects_invalid_stacks_missing_secrets_and_unapproved_switches_without_writes() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let stack = dir.path().join("stack.yml");
    let original = "[mcp_servers.keep]\ncommand='original'\n";
    std::fs::write(&config, original).unwrap();
    for (document, flags, code) in [
        ("schema_version: 2\nservers: {}", vec!["-y"], 4),
        (
            "schema_version: 1\nservers:\n  new:\n    config: {command: new, future: true}",
            vec!["-y"],
            10,
        ),
        (
            "schema_version: 1\nservers:\n  new:\n    config: {command: new, env: {TOKEN: {'$env': MCPSTACK_USE_TOKEN}}}",
            vec!["-y"],
            10,
        ),
        ("schema_version: 1\nservers: {}", vec![], 10),
    ] {
        std::fs::write(&stack, document).unwrap();
        let output = execute(&stack, &config, &flags);
        assert_eq!(output.status.code(), Some(code), "{output:?}");
        assert!(output.stdout.is_empty());
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
        assert!(!dir.path().join("config.toml.~1~").exists());
    }
}

#[test]
#[cfg(any(unix, windows))]
fn default_codex_target_resolves_masked_secrets_and_explicit_target_is_supported() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let stack = dir.path().join("stack.yml");
    std::fs::write(&stack, "schema_version: 1\nservers:\n  new:\n    config: {command: new, env: {TOKEN: {'$env': MCPSTACK_USE_TOKEN}}}\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .arg("use")
        .arg(&stack)
        .arg("-y")
        .env("CODEX_HOME", dir.path())
        .env("MCPSTACK_USE_TOKEN", "fixture-use-secret")
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        servers(&config)["new"]["env"]["TOKEN"].as_str(),
        Some("fixture-use-secret")
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("fixture-use-secret"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-use-secret"));
    let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args(["use", "--client", "codex", "--dry-run"])
        .arg(&stack)
        .arg("--config")
        .arg(&config)
        .env("MCPSTACK_USE_TOKEN", "fixture-use-secret")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[test]
#[cfg(any(unix, windows))]
fn failed_backups_and_invalid_target_configs_preserve_originals() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let stack = dir.path().join("stack.yml");
    std::fs::write(&stack, "schema_version: 1\nservers: {}\n").unwrap();
    let original = "[mcp_servers.keep]\ncommand='original'\n";
    std::fs::write(&config, original).unwrap();
    // A generation beyond u64 prevents automatic numbered backup discovery.
    std::fs::write(
        dir.path().join("config.toml.~18446744073709551616~"),
        "existing backup",
    )
    .unwrap();
    let output = execute(&stack, &config, &["-y"]);
    assert_eq!(output.status.code(), Some(11), "{output:?}");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    assert!(!dir.path().join("config.toml.~1~").exists());
    for original in ["broken = [", "mcp_servers=42", "[mcp_servers]\nserver=42"] {
        std::fs::write(&config, original).unwrap();
        let output = execute(&stack, &config, &["-y"]);
        assert_eq!(output.status.code(), Some(13), "{output:?}");
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
        assert!(!dir.path().join("config.toml.~1~").exists());
    }
}
