use super::*;
use std::net::TcpListener;
use std::thread::{self, JoinHandle};

fn server(status: &str, body: &str) -> (String, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = vec![0; 8192];
        let size = stream.read(&mut request).unwrap();
        // Oversized response tests can close the connection before all bytes are sent.
        let _ = stream.write_all(response.as_bytes());
        String::from_utf8_lossy(&request[..size]).into_owned()
    });
    (url, worker)
}

const RELEASES: &str = r#"{"versions":[
    {"num":"0.9.0","yanked":false},
    {"num":"0.10.0","yanked":false},
    {"num":"5.0.0","yanked":true},
    {"num":"6.0.0-beta.1","yanked":false},
    {"num":"not a version","yanked":false}
]}"#;

#[test]
fn release_order_ignores_yanked_prerelease_invalid_and_build_metadata() {
    assert_eq!(
        latest_stable(serde_json::from_str(RELEASES).unwrap()),
        Some(Version::new(0, 10, 0))
    );
    assert!(!is_newer(
        &Version::parse("1.0.0+foo").unwrap(),
        &Version::parse("1.0.0+bar").unwrap()
    ));
    assert!(!is_newer(&Version::new(1, 0, 0), &Version::new(2, 0, 0)));
    assert!(is_newer(
        &Version::new(1, 0, 0),
        &Version::parse("1.0.0-rc.1").unwrap()
    ));
    assert_eq!(
        latest_stable(
            serde_json::from_str(r#"{"versions":[{"num":"1.0.0-rc.1","yanked":false}]}"#).unwrap()
        ),
        None
    );
}

#[tokio::test]
async fn queries_registry_and_sends_identifiable_user_agent() {
    let (url, worker) = server("200 OK", RELEASES);
    assert_eq!(
        fetch_latest(&url, Duration::from_secs(2)).await.unwrap(),
        Some(Version::new(0, 10, 0))
    );
    let request = worker.join().unwrap().to_lowercase();
    assert!(request.contains(env!("CARGO_PKG_REPOSITORY")));
    assert!(request.contains(&format!(
        "user-agent: mcpstack/{}",
        env!("CARGO_PKG_VERSION")
    )));
}

#[tokio::test]
async fn no_release_differs_from_unavailable_or_invalid_registry() {
    for (status, body, succeeds) in [
        ("404 Not Found", "", true),
        ("200 OK", r#"{"versions":[]}"#, true),
        ("200 OK", "secret invalid JSON", false),
        ("200 OK", r#"{"versions":[{"num":"9.0.0"}]}"#, false),
        ("429 Too Many Requests", "secret server response", false),
        ("500 Server Error", "secret server response", false),
        ("302 Found", "", false),
    ] {
        let (url, worker) = server(status, body);
        let result = fetch_latest(&url, Duration::from_secs(2)).await;
        if succeeds {
            assert_eq!(result.unwrap(), None);
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.code().as_u8(), 15);
            assert!(!error.to_string().contains("secret"));
        }
        worker.join().unwrap();
    }
}

#[tokio::test]
async fn limits_body_size_and_total_request_time() {
    let (url, worker) = server("200 OK", &" ".repeat(MAX_RESPONSE_BYTES + 1));
    assert!(fetch_latest(&url, Duration::from_secs(2)).await.is_err());
    worker.join().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        let (_stream, _) = listener.accept().unwrap();
        thread::sleep(Duration::from_millis(150));
    });
    let started = std::time::Instant::now();
    assert!(fetch_latest(&url, Duration::from_millis(50)).await.is_err());
    assert!(started.elapsed() < Duration::from_secs(1));
    worker.join().unwrap();
}

#[tokio::test]
async fn cache_reuses_results_and_backs_off_after_failures() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("cache/update.json");
    let (url, worker) = server("200 OK", RELEASES);
    let latest = cached_latest(&path, 100, &url).await;
    assert_eq!(latest, Some(Version::new(0, 10, 0)));
    worker.join().unwrap();
    // The server is gone: fresh cache must not request the network again.
    assert_eq!(cached_latest(&path, 101, &url).await, latest);
    let (url, worker) = server("500 Server Error", "");
    assert_eq!(cached_latest(&path, 100 + CHECK_PERIOD, &url).await, None);
    worker.join().unwrap();
    assert_eq!(read_cache(&path).unwrap().checked_at, 100 + CHECK_PERIOD);
    assert_eq!(cached_latest(&path, 101 + CHECK_PERIOD, &url).await, None);
}

#[tokio::test]
async fn malformed_future_or_stale_cache_refreshes_and_unwritable_cache_is_nonfatal() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("update.json");
    for value in [
        "invalid".to_owned(),
        r#"{"checked_at":9999999,"latest":"9.0.0"}"#.to_owned(),
    ] {
        std::fs::write(&path, value).unwrap();
        let (url, worker) = server("200 OK", RELEASES);
        assert_eq!(
            cached_latest(&path, CHECK_PERIOD, &url).await,
            Some(Version::new(0, 10, 0))
        );
        worker.join().unwrap();
    }
    // A file as the parent makes cache writes fail on every supported OS.
    let (url, worker) = server("200 OK", RELEASES);
    assert_eq!(
        cached_latest(&path.join("child.json"), CHECK_PERIOD, &url).await,
        Some(Version::new(0, 10, 0))
    );
    worker.join().unwrap();
}
