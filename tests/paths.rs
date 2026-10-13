use serde_json::{Value, json};
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(
    client: &str,
    operation: &str,
    cwd: &Path,
    stack: &Path,
    config: &Path,
    flags: &[&str],
) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args(["--non-interactive", "--color", "never", client, operation])
        .arg(stack)
        .arg("--config")
        .arg(config)
        .args(flags)
        .current_dir(cwd)
        .env_remove("RUST_LOG")
        .env("MCPSTACK_PATH_COMMAND", "./from-environment")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn success(output: Output) -> String {
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

fn read_servers(client: &str, config: &Path) -> Value {
    let text = std::fs::read_to_string(config).unwrap();
    let (root, key): (Value, _) = if client == "codex" {
        (
            serde_json::to_value(toml::from_str::<toml::Value>(&text).unwrap()).unwrap(),
            "mcp_servers",
        )
    } else {
        (serde_json::from_str(&text).unwrap(), "mcpServers")
    };
    root[key].clone()
}

#[test]
fn stack_paths_and_previews_are_independent_of_invocation_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let bundle = root.join("bundle with spaces");
    let first = root.join("first");
    let second = root.join("second");
    for path in [&bundle, &first, &second] {
        std::fs::create_dir(path).unwrap();
    }
    let stack = bundle.join("stack.yml");
    let relative = Path::new("../bundle with spaces/stack.yml");
    let absolute = root.join("absolute/server");
    let document = json!({"schema_version":1,"servers":{
        "portable":{"transport":{"type":"stdio","command":"./bin/portable","args":["./argument.js"],"env":{"DATA":"../data"}}},
        "native":{"config":{"command":"tools/native","args":["../script.js"],"env":{"DATA":"./data"}}},
        "parent":{"transport":{"type":"stdio","command":"../shared/server"}},
        "absolute":{"transport":{"type":"stdio","command":absolute}},
        "npx":{"transport":{"type":"stdio","command":"npx"}},
        "uvx":{"transport":{"type":"stdio","command":"uvx"}},
        "docker":{"transport":{"type":"stdio","command":"docker"}},
        "runtime":{"transport":{"type":"stdio","command":"${MCPSTACK_PATH_RUNTIME}/server"}},
        "reference":{"config":{"command":{"$env":"MCPSTACK_PATH_COMMAND"}}},
        "remote":{"transport":{"type":"http","url":"https://example.com/mcp"}}
    }}).to_string();
    std::fs::write(&stack, &document).unwrap();
    for client in ["codex", "claude"] {
        for operation in ["import", "use"] {
            let config = root.join(format!("{client}-{operation}.config"));
            let empty = if client == "codex" { "" } else { "{}" };
            std::fs::write(&config, empty).unwrap();
            for preview_operation in ["diff", operation] {
                let flags: &[&str] = if preview_operation == "diff" {
                    &[]
                } else {
                    &["--dry-run", "-y"]
                };
                let preview_a = success(run(
                    client,
                    preview_operation,
                    &first,
                    relative,
                    &config,
                    flags,
                ));
                let preview_b = success(run(
                    client,
                    preview_operation,
                    &second,
                    relative,
                    &config,
                    flags,
                ));
                assert_eq!(preview_a, preview_b);
                let target = format!(
                    "Target config: {}\nMCP server changes:",
                    config.to_string_lossy().escape_debug()
                );
                assert!(preview_a.contains(&target), "{preview_a}");
                assert_eq!(std::fs::read_to_string(&config).unwrap(), empty);
            }
            success(run(client, operation, &first, relative, &config, &["-y"]));
            let servers = read_servers(client, &config);
            for (name, path) in [
                ("portable", "bin/portable"),
                ("native", "tools/native"),
                ("parent", "../shared/server"),
            ] {
                let expected = std::path::absolute(bundle.join(path)).unwrap();
                assert_eq!(
                    Path::new(servers[name]["command"].as_str().unwrap()),
                    expected
                );
            }
            assert_eq!(
                Path::new(servers["absolute"]["command"].as_str().unwrap()),
                absolute
            );
            for name in ["npx", "uvx", "docker"] {
                assert_eq!(servers[name]["command"], name);
            }
            assert_eq!(
                servers["runtime"]["command"],
                "${MCPSTACK_PATH_RUNTIME}/server"
            );
            assert_eq!(servers["reference"]["command"], "./from-environment");
            assert_eq!(servers["portable"]["args"], json!(["./argument.js"]));
            assert_eq!(servers["portable"]["env"]["DATA"], "../data");
            assert_eq!(servers["native"]["args"], json!(["../script.js"]));
            assert_eq!(servers["native"]["env"]["DATA"], "./data");
            assert_eq!(servers["remote"]["url"], "https://example.com/mcp");
            let before = std::fs::read(&config).unwrap();
            let diff = success(run(client, "diff", &second, relative, &config, &[]));
            assert!(!diff.contains("@@"), "{diff}");
            success(run(client, operation, &second, relative, &config, &["-y"]));
            assert_eq!(std::fs::read(&config).unwrap(), before);
        }
    }
    assert_eq!(std::fs::read_to_string(&stack).unwrap(), document);
    assert_eq!(std::fs::read_dir(&bundle).unwrap().count(), 1);
}

#[test]
fn codex_cwd_is_stack_relative_independently_of_the_command() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let bundle = root.join("bundle");
    std::fs::create_dir(&bundle).unwrap();
    let stack = bundle.join("stack.yml");
    let config = root.join("config.toml");
    let absolute = root.join("absolute-cwd");
    std::fs::write(
        &stack,
        json!({"schema_version":1,"servers":{
            "portable":{"transport":{"type":"stdio","command":"./bin/server","cwd":"workspace"}},
            "native":{"config":{"command":"npx","cwd":"../workspace"}},
            "current":{"config":{"command":"tool","cwd":"."}},
            "absolute":{"config":{"command":"tool","cwd":absolute}}
        }})
        .to_string(),
    )
    .unwrap();
    for operation in ["import", "use"] {
        std::fs::write(&config, "").unwrap();
        success(run("codex", operation, &root, &stack, &config, &["-y"]));
        let servers = read_servers("codex", &config);
        assert_eq!(
            Path::new(servers["portable"]["cwd"].as_str().unwrap()),
            bundle.join("workspace")
        );
        assert_eq!(
            Path::new(servers["portable"]["command"].as_str().unwrap()),
            bundle.join("bin/server")
        );
        assert_eq!(
            Path::new(servers["native"]["cwd"].as_str().unwrap()),
            std::path::absolute(bundle.join("../workspace")).unwrap()
        );
        assert_eq!(
            Path::new(servers["current"]["cwd"].as_str().unwrap()),
            bundle
        );
        assert_eq!(
            Path::new(servers["absolute"]["cwd"].as_str().unwrap()),
            absolute
        );
    }
    // Path resolution must not bypass an adapter's unsupported-field checks.
    std::fs::write(&config, "{}").unwrap();
    let output = run("claude", "use", &root, &stack, &config, &["--dry-run"]);
    assert_eq!(output.status.code(), Some(10), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("cwd"));
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "{}");
}

#[cfg(unix)]
#[test]
fn symlinked_stack_uses_the_links_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let originals = root.join("originals");
    let links = root.join("links");
    std::fs::create_dir(&originals).unwrap();
    std::fs::create_dir(&links).unwrap();
    let original = originals.join("stack.yml");
    let link = links.join("stack.yml");
    std::fs::write(
        &original,
        "schema_version: 1\nservers:\n  example:\n    config: {command: ./server}\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(&original, &link).unwrap();
    for client in ["codex", "claude"] {
        let config = root.join(client);
        success(run(client, "use", &root, &link, &config, &["-y"]));
        let servers = read_servers(client, &config);
        assert_eq!(
            Path::new(servers["example"]["command"].as_str().unwrap()),
            links.join("server")
        );
    }
}

#[cfg(windows)]
#[test]
fn windows_paths_use_native_separators_and_reject_drive_relative_paths() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let stack = root.join("stack.yml");
    for client in ["codex", "claude"] {
        let config = root.join(client);
        std::fs::write(
            &stack,
            json!({"schema_version":1,"servers":{
                "relative":{"transport":{"type":"stdio","command":r".\bin\server.exe"}},
                "absolute":{"transport":{"type":"stdio","command":r"C:\tools\server.exe"}},
                "unc":{"transport":{"type":"stdio","command":r"\\host\share\server.exe"}}
            }})
            .to_string(),
        )
        .unwrap();
        success(run(client, "use", &root, &stack, &config, &["-y"]));
        let servers = read_servers(client, &config);
        assert_eq!(
            Path::new(servers["relative"]["command"].as_str().unwrap()),
            root.join(r"bin\server.exe")
        );
        assert_eq!(servers["absolute"]["command"], r"C:\tools\server.exe");
        assert_eq!(servers["unc"]["command"], r"\\host\share\server.exe");
        let before = std::fs::read(&config).unwrap();
        std::fs::write(
            &stack,
            "schema_version: 1\nservers:\n  bad:\n    config: {command: 'C:server.exe'}\n",
        )
        .unwrap();
        let output = run(client, "use", &root, &stack, &config, &["-y"]);
        assert_eq!(output.status.code(), Some(4), "{output:?}");
        assert_eq!(std::fs::read(&config).unwrap(), before);
    }
}

#[test]
fn windows_absolute_paths_are_preserved_on_every_host() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let stack = root.join("stack.yml");
    let paths = [
        r"C:\tools\server.exe",
        "C:/tools/server.exe",
        r"\\host\share\server.exe",
        "//host/share/server.exe",
        r"\\?\C:\tools\server.exe",
        r"\\?\UNC\host\share\server.exe",
        r"\\.\device\server",
    ];
    let definitions: serde_json::Map<_, _> = paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            (
                format!("server{index}"),
                json!({"transport":{"type":"stdio","command":path}}),
            )
        })
        .collect();
    std::fs::write(
        &stack,
        json!({"schema_version":1,"servers":definitions}).to_string(),
    )
    .unwrap();
    for client in ["codex", "claude"] {
        let config = root.join(client);
        success(run(client, "use", &root, &stack, &config, &["-y"]));
        let servers = read_servers(client, &config);
        for (index, path) in paths.iter().enumerate() {
            assert_eq!(servers[format!("server{index}")]["command"], *path);
        }
    }
}

#[test]
fn exported_relative_paths_round_trip_without_rebasing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let bundle = root.join("bundle");
    let moved = root.join("moved");
    let other = root.join("other");
    for path in [&bundle, &moved, &other] {
        std::fs::create_dir(path).unwrap();
    }
    for client in ["codex", "claude"] {
        let config = root.join(format!("{client}.config"));
        let mut servers = json!({
            "relative": {"command":"./bin/server", "args":["./argument.js"]},
            "parent": {"command":"../shared/server"},
            "bare": {"command":"npx"}
        });
        if client == "codex" {
            servers["absolute_cwd"] = json!({"command":"./bin/server", "cwd":root.join("app")});
            servers["relative_cwd"] = json!({"command":"./bin/server", "cwd":"./workspace"});
        } else {
            for server in servers.as_object_mut().unwrap().values_mut() {
                server["type"] = json!("stdio");
            }
        }
        let original = if client == "codex" {
            toml::to_string(&json!({"mcp_servers":servers})).unwrap()
        } else {
            serde_json::to_string_pretty(&json!({"mcpServers":servers})).unwrap()
        };
        std::fs::write(&config, &original).unwrap();
        let stack = bundle.join(format!("{client}.yml"));
        success(run(client, "export", &root, &stack, &config, &[]));
        let yaml = std::fs::read_to_string(&stack).unwrap();
        assert!(yaml.contains("path_resolution: client"), "{yaml}");
        // Relocation and a different invoking directory must not change native paths.
        let relocated = moved.join(format!("{client}.yml"));
        std::fs::rename(&stack, &relocated).unwrap();
        for operation in ["diff", "import", "use"] {
            let flags: &[&str] = if operation == "diff" {
                &[]
            } else {
                &["--dry-run", "-y"]
            };
            let preview = success(run(client, operation, &other, &relocated, &config, flags));
            assert!(!preview.contains("@@"), "{preview}");
            assert!(!preview.contains("~ Modified:"), "{preview}");
            if operation != "diff" {
                success(run(client, operation, &other, &relocated, &config, &["-y"]));
            }
            assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
        }
        // The marker also survives show redirected to a stack, and applies on either client.
        let shown = success(
            Command::new(env!("CARGO_BIN_EXE_mcpstack"))
                .args([
                    "--non-interactive",
                    client,
                    "show",
                    "--server",
                    "relative",
                    "--config",
                ])
                .arg(&config)
                .output()
                .unwrap(),
        );
        std::fs::write(&relocated, shown).unwrap();
        for target in ["codex", "claude"] {
            let destination = root.join(format!("{client}-to-{target}.config"));
            success(run(
                target,
                "use",
                &other,
                &relocated,
                &destination,
                &["-y"],
            ));
            let restored = read_servers(target, &destination);
            assert_eq!(restored["relative"]["command"], "./bin/server");
            assert_eq!(restored["relative"]["args"], json!(["./argument.js"]));
            assert!(restored["relative"].get("path_resolution").is_none());
        }
    }
}

#[cfg(unix)]
#[test]
fn target_summary_escapes_control_characters_in_filenames() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let stack = root.join("stack.yml");
    std::fs::write(
        &stack,
        "schema_version: 1\nservers:\n  example:\n    config: {command: tool}\n",
    )
    .unwrap();
    for client in ["codex", "claude"] {
        let config = root.join(format!("{client}\x1b[2J\nforged-summary\r\t.config"));
        for operation in ["diff", "import", "use"] {
            let preview = success(run(
                client,
                operation,
                &root,
                &stack,
                &config,
                if operation == "diff" {
                    &["--no-color"]
                } else {
                    &["--dry-run", "-y"]
                },
            ));
            let expected = format!(
                "Target config: {}\nMCP server changes:",
                config.to_string_lossy().escape_debug()
            );
            assert!(preview.contains(&expected), "{preview:?}");
            if operation == "diff" {
                let label = config.to_string_lossy().escape_debug().to_string();
                assert!(preview.contains(&format!("+++ {label}\n")), "{preview:?}");
            }
            assert!(!preview.contains(['\x1b', '\r', '\t']), "{preview:?}");
            assert!(!preview.contains("\nforged-summary"), "{preview:?}");
            assert!(!config.exists());
        }
    }
}
