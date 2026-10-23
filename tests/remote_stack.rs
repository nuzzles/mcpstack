use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const STACK: &str =
    "schema_version: 1\nservers:\n  example:\n    transport: {type: stdio, command: tool}\n";

fn accept_connection(listener: &TcpListener) -> Option<TcpStream> {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                return Some(stream);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("server accept failed: {error}"),
        }
    }
}

fn request_headers(stream: &TcpStream) -> Vec<String> {
    let mut reader = BufReader::new(stream);
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        assert_ne!(reader.read_line(&mut line).unwrap(), 0);
        if line == "\r\n" {
            return headers;
        }
        headers.push(line);
    }
}

fn serve(status: &str, body: &[u8]) -> (String, std::thread::JoinHandle<bool>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let status = status.to_owned();
    let body = body.to_vec();
    let thread = std::thread::spawn(move || {
        let Some(mut stream) = accept_connection(&listener) else {
            return false;
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = [0; 1024];
        let read = stream.read(&mut request).unwrap();
        assert!(request[..read].starts_with(b"GET /stack.yaml?token=fixture-url-secret HTTP/1.1"));
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        stream.write_all(&body).unwrap_or(());
        true
    });
    (
        format!("http://{address}/stack.yaml?token=fixture-url-secret"),
        thread,
    )
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args(["--non-interactive", "--color", "never"])
        .args(args)
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn config_text(client: &str) -> &'static str {
    if client == "codex" { "" } else { "{}" }
}

#[cfg(unix)]
#[test]
fn direct_import_fetches_a_multi_harness_url_once() {
    let dir = tempfile::tempdir().unwrap();
    let codex_home = dir.path().join("codex");
    std::fs::create_dir(&codex_home).unwrap();
    let codex_config = codex_home.join("config.toml");
    let claude_config = dir.path().join(".claude.json");
    std::fs::write(&codex_config, "").unwrap();
    std::fs::write(&claude_config, "{}").unwrap();
    // The fixture closes after one response, like a one-use signed stack URL.
    let (url, server) = serve("200 OK", STACK.as_bytes());
    let result = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .args([
            "--non-interactive",
            "import",
            &url,
            "--harness",
            "codex",
            "--harness",
            "claude",
            "-y",
        ])
        .env("CODEX_HOME", &codex_home)
        .env("HOME", dir.path())
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(server.join().unwrap());
    assert!(result.status.success(), "{result:?}");
    assert!(
        std::fs::read_to_string(&codex_config)
            .unwrap()
            .contains("example")
    );
    assert!(
        std::fs::read_to_string(&claude_config)
            .unwrap()
            .contains("example")
    );
}

#[test]
fn http_stack_works_for_validate_diff_import_and_use_in_both_clients() {
    let dir = tempfile::tempdir().unwrap();
    let (url, server) = serve("200 OK", STACK.as_bytes());
    let validated = run(&["validate", &url]);
    assert!(server.join().unwrap());
    assert!(validated.status.success(), "{validated:?}");
    assert_eq!(
        String::from_utf8(validated.stdout).unwrap(),
        format!(
            "Stack URL: {}\nValid stack (schema 1, 1 server(s)).\n",
            url.escape_debug()
        )
    );
    for client in ["codex", "claude"] {
        for operation in ["diff", "import", "use"] {
            let config = dir.path().join(format!("{client}-{operation}.config"));
            std::fs::write(&config, config_text(client)).unwrap();
            let (url, server) = serve("200 OK", STACK.as_bytes());
            let mut args = vec![
                client,
                operation,
                url.as_str(),
                "--config",
                config.to_str().unwrap(),
            ];
            if operation != "diff" {
                args.push("-y");
            }
            let output = run(&args);
            assert!(server.join().unwrap());
            assert!(output.status.success(), "{client} {operation}: {output:?}");
            assert!(!String::from_utf8_lossy(&output.stdout).contains("fixture-url-secret"));
            assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-url-secret"));
            let written = std::fs::read_to_string(&config).unwrap();
            if operation == "diff" {
                assert_eq!(written, config_text(client));
            } else {
                assert!(written.contains("example"), "{written}");
            }
        }
    }
}

#[test]
fn redirects_do_not_send_source_query_in_referer() {
    let target = TcpListener::bind("127.0.0.1:0").unwrap();
    let target_url = format!("http://{}/stack.yaml", target.local_addr().unwrap());
    let source = TcpListener::bind("127.0.0.1:0").unwrap();
    let source_url = format!(
        "http://{}/stack.yaml?token=fixture-url-secret",
        source.local_addr().unwrap()
    );
    let source_server = std::thread::spawn(move || {
        let mut stream = accept_connection(&source).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let headers = request_headers(&stream);
        assert!(headers[0].starts_with("GET /stack.yaml?token=fixture-url-secret HTTP/1.1"));
        write!(
            stream,
            "HTTP/1.1 302 Found\r\nLocation: {target_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
    });
    let target_server = std::thread::spawn(move || {
        let mut stream = accept_connection(&target).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let headers = request_headers(&stream);
        assert!(headers[0].starts_with("GET /stack.yaml HTTP/1.1"));
        assert!(
            headers
                .iter()
                .all(|line| !line.to_ascii_lowercase().starts_with("referer:")),
            "redirected request must omit Referer"
        );
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{STACK}",
            STACK.len()
        )
        .unwrap();
    });
    let output = run(&["validate", &source_url]);
    source_server.join().unwrap();
    target_server.join().unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn remote_failures_leave_client_config_untouched() {
    let dir = tempfile::tempdir().unwrap();
    for (status, body, code, message) in [
        ("404 Not Found", STACK.as_bytes().to_vec(), 3, "HTTP 404"),
        ("200 OK", b"not yaml".to_vec(), 4, "INVALID_STACK"),
        ("200 OK", vec![b' '; 4 * 1024 * 1024 + 1], 3, "4 MiB"),
        (
            "200 OK",
            b"schema_version: 1\nservers:\n  example:\n    transport: {type: stdio, command: ./bin/tool}\n".to_vec(),
            4,
            "stack-relative",
        ),
    ] {
        let config = dir.path().join("config.toml");
        std::fs::write(&config, "# preserved\n").unwrap();
        let (url, server) = serve(status, &body);
        let output = run(&["codex", "import", &url, "--config", config.to_str().unwrap(), "-y"]);
        assert!(server.join().unwrap());
        assert_eq!(output.status.code(), Some(code), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(diagnostic.contains(message), "{diagnostic}");
        assert!(!diagnostic.contains("fixture-url-secret"));
        assert_eq!(std::fs::read_to_string(&config).unwrap(), "# preserved\n");
        assert!(!Path::new(&format!("{}.~1~", config.display())).exists());
    }
}

#[test]
fn remote_stack_can_keep_client_relative_paths_when_explicit() {
    let dir = tempfile::tempdir().unwrap();
    for client in ["codex", "claude"] {
        let document = if client == "codex" {
            "schema_version: 1\nservers:\n  example:\n    transport: {type: stdio, command: ./bin/tool, cwd: ./workspace, path_resolution: client}\n"
        } else {
            "schema_version: 1\nservers:\n  example:\n    transport: {type: stdio, command: ./bin/tool, path_resolution: client}\n"
        };
        let config = dir.path().join(format!("{client}.config"));
        std::fs::write(&config, config_text(client)).unwrap();
        let (url, server) = serve("200 OK", document.as_bytes());
        let output = run(&[
            client,
            "use",
            &url,
            "--config",
            config.to_str().unwrap(),
            "-y",
        ]);
        assert!(server.join().unwrap());
        assert!(output.status.success(), "{client}: {output:?}");
        let written = std::fs::read_to_string(&config).unwrap();
        assert!(written.contains("./bin/tool"), "{written}");
        if client == "codex" {
            assert!(written.contains("./workspace"), "{written}");
        }
    }
}

#[test]
fn unsupported_and_malformed_urls_are_rejected_without_echoing_values() {
    for (source, message) in [
        (
            "ftp://fixture-url-secret.invalid/stack.yaml",
            "HTTP or HTTPS",
        ),
        (
            "https://fixture-url-secret.invalid:bad/stack.yaml",
            "Invalid HTTP(S)",
        ),
    ] {
        let output = run(&["validate", source]);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(diagnostic.contains(message), "{diagnostic}");
        assert!(!diagnostic.contains("fixture-url-secret"));
    }
}

#[cfg(unix)]
#[test]
fn local_stack_paths_with_url_syntax_are_read_as_files() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("name:");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("stack.yaml"), STACK).unwrap();
    let absolute = format!("{}/name://stack.yaml", dir.path().display());
    for source in [&absolute, "./name://stack.yaml"] {
        assert!(dir.path().join(source).is_file());
        let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
            .args(["validate", source])
            .current_dir(dir.path())
            .env_remove("RUST_LOG")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn local_stack_paths_can_contain_non_utf8_bytes() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let dir = tempfile::tempdir().unwrap();
    let file = dir
        .path()
        .join(OsString::from_vec(b"stack-\xff.yaml".to_vec()));
    std::fs::write(&file, STACK).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_mcpstack"))
        .arg("validate")
        .arg(&file)
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("Valid stack"));
}
