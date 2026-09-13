use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::Duration;

use super::*;
use crate::registry::RegistryConfig;

fn spawn_server(config: RegistryConfig) -> (String, PathBufGuard) {
    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(dir.path(), config).unwrap();
    let server = RegistryServer::bind("127.0.0.1:0", registry).unwrap();
    let addr = server.local_addr().unwrap();
    thread::spawn(move || {
        let _ = server.run(None);
    });
    (format!("http://{addr}"), PathBufGuard(dir))
}

/// Keeps the tempdir alive for the server thread's lifetime (the server
/// thread is detached — the guard just controls when cleanup runs). The
/// field is never read, only held for its `Drop` impl.
#[allow(dead_code)]
struct PathBufGuard(tempfile::TempDir);

fn generous_config() -> RegistryConfig {
    RegistryConfig {
        max_queue_depth_per_repo: 1_000,
        max_pushes_per_minute_per_repo: 1_000,
    }
}

/// Raw client using the same one-shot-connection protocol the real
/// `HubClient` speaks — exercised directly here rather than importing
/// `crate::client`, since this is testing the *server's* wire behavior.
fn raw_request(
    base_url: &str,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: &[u8],
) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let authority = base_url.strip_prefix("http://").unwrap();
    let mut stream = TcpStream::connect(authority).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n");
    for (name, value) in headers {
        req.push_str(&format!("{name}: {value}\r\n"));
    }
    if method == "PUT" {
        req.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes()).unwrap();
    if method == "PUT" {
        stream.write_all(body).unwrap();
    }
    stream.flush().unwrap();

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.lines();
    let status: u16 = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|l| {
            l.split_once(':')
                .map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
        })
        .collect();
    (status, headers, raw[split + 4..].to_vec())
}

#[test]
fn parse_snapshot_path_extracts_repo_and_sha_ignoring_any_prefix() {
    assert_eq!(
        parse_snapshot_path("/snapshots/my-repo/abc123.tar.zst"),
        Some(("my-repo".to_string(), "abc123".to_string()))
    );
    assert_eq!(
        parse_snapshot_path("/weave/snapshots/my-repo/abc123.tar.zst"),
        Some(("my-repo".to_string(), "abc123".to_string()))
    );
    assert_eq!(parse_snapshot_path("/health"), None);
    assert_eq!(parse_snapshot_path("/snapshots/"), None);
}

/// Security regression: a crafted `repo_id`/sha carrying `..`/`/` must
/// never reach `Registry` as a usable path component — the specific
/// path-traversal vector a malicious `PUT`/`GET` could otherwise use to
/// read or write arbitrary files on the host.
#[test]
fn parse_snapshot_path_rejects_path_traversal_attempts() {
    assert_eq!(
        parse_snapshot_path("/snapshots/../../etc/sha1.tar.zst"),
        None
    );
    assert_eq!(
        parse_snapshot_path("/snapshots/my-repo/../../../etc/passwd.tar.zst"),
        None
    );
    assert_eq!(
        parse_snapshot_path("/snapshots/my-repo/..%2f..%2fetc%2fpasswd.tar.zst"),
        None,
        "a raw percent-encoded traversal string must still fail the charset check"
    );
    assert_eq!(
        parse_snapshot_path("/snapshots/./my-repo/sha1.tar.zst"),
        None
    );
    assert_eq!(
        parse_snapshot_path("/snapshots/my..repo/sha1.tar.zst"),
        Some(("my..repo".to_string(), "sha1".to_string())),
        "a dot-containing but non-'..'-exact component is fine"
    );
}

#[test]
fn push_and_pull_with_a_traversal_repo_id_are_refused_end_to_end() {
    let (base, _guard) = spawn_server(generous_config());
    let (status, _headers, _body) = raw_request(
        &base,
        "PUT",
        "/snapshots/../../etc/sha1.tar.zst",
        &[],
        b"malicious payload",
    );
    assert_eq!(
        status, 404,
        "a traversal path must never be routed to a push"
    );

    let (status, _headers, _body) = raw_request(
        &base,
        "GET",
        "/snapshots/../../etc/passwd.tar.zst",
        &[],
        b"",
    );
    assert_eq!(
        status, 404,
        "a traversal path must never be routed to a pull"
    );
}

#[test]
fn push_then_pull_round_trips_over_real_tcp() {
    let (base, _guard) = spawn_server(generous_config());
    let (status, _headers, _body) = raw_request(
        &base,
        "PUT",
        "/snapshots/my-repo/sha1.tar.zst",
        &[("X-Weave-Retention", "20".to_string())],
        b"snapshot bytes",
    );
    assert_eq!(status, 202);

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let (status, _headers, body) =
            raw_request(&base, "GET", "/snapshots/my-repo/sha1.tar.zst", &[], b"");
        if status == 200 {
            assert_eq!(body, b"snapshot bytes");
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "worker never committed"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn push_with_stale_base_sha_returns_409() {
    let (base, _guard) = spawn_server(generous_config());
    raw_request(&base, "PUT", "/snapshots/my-repo/sha1.tar.zst", &[], b"v1");
    let (status, _headers, _body) = raw_request(
        &base,
        "PUT",
        "/snapshots/my-repo/sha2.tar.zst",
        &[("X-Weave-Base-Sha", "wrong".to_string())],
        b"v2",
    );
    assert_eq!(status, 409);
}

#[test]
fn push_past_the_watermark_returns_429_with_retry_after() {
    let (base, _guard) = spawn_server(RegistryConfig {
        max_queue_depth_per_repo: 0,
        max_pushes_per_minute_per_repo: 1_000,
    });
    raw_request(&base, "PUT", "/snapshots/my-repo/sha1.tar.zst", &[], b"v1");
    let (status, headers, _body) =
        raw_request(&base, "PUT", "/snapshots/my-repo/sha2.tar.zst", &[], b"v2");
    assert_eq!(status, 429);
    assert!(
        headers
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case("retry-after"))
    );
}

#[test]
fn pull_of_missing_snapshot_is_404() {
    let (base, _guard) = spawn_server(generous_config());
    let (status, _headers, _body) =
        raw_request(&base, "GET", "/snapshots/nope/sha1.tar.zst", &[], b"");
    assert_eq!(status, 404);
}

#[test]
fn unsupported_method_is_405() {
    let (base, _guard) = spawn_server(generous_config());
    let (status, _headers, _body) =
        raw_request(&base, "DELETE", "/snapshots/my-repo/sha1.tar.zst", &[], b"");
    assert_eq!(status, 405);
}

#[test]
fn unrecognized_path_is_404() {
    let (base, _guard) = spawn_server(generous_config());
    let (status, _headers, _body) = raw_request(&base, "GET", "/health", &[], b"");
    assert_eq!(status, 404);
}

#[test]
fn run_stops_accepting_after_max_connections() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(dir.path(), generous_config()).unwrap();
    let server = RegistryServer::bind("127.0.0.1:0", registry).unwrap();
    let addr = server.local_addr().unwrap();

    let client = thread::spawn(move || {
        raw_request(
            &format!("http://{addr}"),
            "PUT",
            "/snapshots/my-repo/sha1.tar.zst",
            &[],
            b"v1",
        )
    });
    server.run(Some(1)).unwrap();
    let (status, ..) = client.join().unwrap();
    assert_eq!(status, 202);
}

#[test]
fn a_connection_closed_before_any_bytes_does_not_disturb_later_requests() {
    let (base, _guard) = spawn_server(generous_config());
    let authority = base.strip_prefix("http://").unwrap();
    drop(TcpStream::connect(authority).unwrap());

    let (status, _headers, _body) = raw_request(&base, "GET", "/snapshots/x/y.tar.zst", &[], b"");
    assert_eq!(status, 404, "server must still answer normally afterward");
}

#[cfg(feature = "hub-canvas")]
#[test]
fn canvas_endpoint_is_404_before_any_push_and_200_after_commit() {
    use weave_graph_core::{Edge, Node, Storage};
    use weave_graph_store_sqlite::SqliteStorage;

    let snapshot_dir = tempfile::tempdir().unwrap();
    let db_path = snapshot_dir.path().join("graph.db");
    {
        let mut storage = SqliteStorage::open(&db_path).unwrap();
        storage
            .upsert_node(&Node {
                id: 1,
                repo_id: "r".into(),
                path: "a.rs".into(),
                symbol: "s1".into(),
                kind: "function".into(),
                line_start: 1,
                line_end: 2,
                signature: String::new(),
            })
            .unwrap();
        storage
            .upsert_node(&Node {
                id: 2,
                repo_id: "r".into(),
                path: "b.rs".into(),
                symbol: "s2".into(),
                kind: "function".into(),
                line_start: 1,
                line_end: 2,
                signature: String::new(),
            })
            .unwrap();
        storage
            .upsert_edge(&Edge {
                id: 0,
                source_id: 1,
                target_id: 2,
                kind: "CALLS_EXACT".into(),
                weight: 1.0,
            })
            .unwrap();
    }
    let bytes = std::fs::read(&db_path).unwrap();

    let (base, _guard) = spawn_server(generous_config());
    let (status, _headers, _body) = raw_request(&base, "GET", "/repos/my-repo/canvas", &[], b"");
    assert_eq!(status, 404);

    raw_request(&base, "PUT", "/snapshots/my-repo/sha1.tar.zst", &[], &bytes);

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let (status, headers, body) = raw_request(&base, "GET", "/repos/my-repo/canvas", &[], b"");
        if status == 200 {
            assert!(
                headers
                    .iter()
                    .any(|(n, v)| n.eq_ignore_ascii_case("content-type")
                        && v == "application/json")
            );
            let text = String::from_utf8(body).unwrap();
            assert!(text.contains("\"nodes\""), "{text}");
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "worker never committed"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(feature = "hub-canvas")]
#[test]
fn mesh_canvas_endpoint_stitches_multiple_repos_in_one_call() {
    use weave_graph_core::{Node, Storage};
    use weave_graph_store_sqlite::SqliteStorage;

    let snapshot_bytes = |file: &str| -> Vec<u8> {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("graph.db");
        {
            let mut storage = SqliteStorage::open(&db_path).unwrap();
            storage
                .upsert_node(&Node {
                    id: 1,
                    repo_id: "r".into(),
                    path: file.into(),
                    symbol: "s1".into(),
                    kind: "function".into(),
                    line_start: 1,
                    line_end: 2,
                    signature: String::new(),
                })
                .unwrap();
        }
        std::fs::read(&db_path).unwrap()
    };

    let (base, _guard) = spawn_server(generous_config());
    raw_request(
        &base,
        "PUT",
        "/snapshots/repo-a/sha1.tar.zst",
        &[],
        &snapshot_bytes("a.rs"),
    );
    raw_request(
        &base,
        "PUT",
        "/snapshots/repo-b/sha1.tar.zst",
        &[],
        &snapshot_bytes("b.rs"),
    );

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let (status, _headers, body) = raw_request(
            &base,
            "GET",
            "/mesh/canvas/repo-a,repo-b,repo-never-published",
            &[],
            b"",
        );
        if status == 200 {
            let text = String::from_utf8(body).unwrap();
            assert!(text.contains("# repo-a"), "{text}");
            assert!(text.contains("# repo-b"), "{text}");
            assert!(!text.contains("repo-never-published"), "{text}");
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "worker never committed both repos"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

/// SSRF guard, exercised through the real HTTP route: a loopback or
/// RFC1918 target must never be accepted for registration — the registry
/// itself is what would dial it on the next commit, not the caller.
#[cfg(feature = "hub-webhooks")]
#[test]
fn webhook_endpoint_rejects_a_private_target_and_accepts_a_public_one() {
    let (base, _guard) = spawn_server(generous_config());

    let (status, _headers, body) = raw_request(
        &base,
        "PUT",
        "/repos/my-repo/webhook",
        &[],
        b"http://127.0.0.1:9/hook",
    );
    assert_ne!(status, 200, "{}", String::from_utf8_lossy(&body));

    // A public IP literal — never actually dialed by this test, `PUT`
    // .../webhook only resolves-and-classifies it, which needs no DNS
    // lookup for an IP literal, so this stays fast and network-independent.
    let (status, _headers, _body) = raw_request(
        &base,
        "PUT",
        "/repos/my-repo/webhook",
        &[],
        b"http://8.8.8.8/hook",
    );
    assert_eq!(status, 200);
}

/// HUB-02: once a token is configured, every route — not just snapshot
/// push/pull — must refuse an unauthenticated or wrong-token request.
#[test]
fn a_configured_auth_token_rejects_requests_without_it() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(dir.path(), generous_config()).unwrap();
    let server =
        RegistryServer::bind_with_token("127.0.0.1:0", registry, Some("s3cr3t".to_string()))
            .unwrap();
    let addr = server.local_addr().unwrap();
    thread::spawn(move || {
        let _ = server.run(None);
    });
    let base = format!("http://{addr}");

    let (status, ..) = raw_request(&base, "GET", "/snapshots/x/y.tar.zst", &[], b"");
    assert_eq!(status, 401, "no Authorization header at all");

    let (status, ..) = raw_request(
        &base,
        "GET",
        "/snapshots/x/y.tar.zst",
        &[("Authorization", "Bearer wrong".to_string())],
        b"",
    );
    assert_eq!(status, 401, "wrong token");

    let (status, ..) = raw_request(
        &base,
        "GET",
        "/snapshots/x/y.tar.zst",
        &[("Authorization", "Bearer s3cr3t".to_string())],
        b"",
    );
    assert_eq!(
        status, 404,
        "correct token reaches routing (no such snapshot)"
    );
}

#[test]
fn a_server_bound_without_a_token_stays_unauthenticated() {
    let (base, _guard) = spawn_server(generous_config());
    let (status, ..) = raw_request(&base, "GET", "/snapshots/x/y.tar.zst", &[], b"");
    assert_eq!(
        status, 404,
        "no token configured means no auth check at all"
    );
}

#[test]
fn a_put_whose_body_is_shorter_than_content_length_does_not_hang_the_server() {
    let (base, _guard) = spawn_server(generous_config());
    let authority = base.strip_prefix("http://").unwrap();
    {
        let mut stream = TcpStream::connect(authority).unwrap();
        stream
            .write_all(
                b"PUT /snapshots/my-repo/sha1.tar.zst HTTP/1.1\r\nContent-Length: 100\r\n\r\nshort",
            )
            .unwrap();
        // Dropped here without sending the remaining 95 claimed bytes.
    }

    let (status, _headers, _body) = raw_request(&base, "GET", "/snapshots/x/y.tar.zst", &[], b"");
    assert_eq!(status, 404, "server must still answer normally afterward");
}
