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
