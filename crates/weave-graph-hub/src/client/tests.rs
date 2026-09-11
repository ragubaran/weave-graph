use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;

/// A one-request fake hub: accepts a single connection, replies with the
/// canned status/headers/body, then shuts down. Tests bind loopback only —
/// no real network anywhere.
struct FakeHub {
    status: u16,
    headers: &'static [&'static str],
    body: &'static [u8],
}

/// Reads headers plus, per `Content-Length`, the full body before returning
/// — a PUT's payload can arrive in a separate TCP segment from its headers,
/// and responding (and dropping the stream) before it's fully drained can
/// reset the still-writing client's connection instead of closing cleanly.
fn read_full_request(stream: &mut std::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let n = stream.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            return String::from_utf8_lossy(&buf).into_owned();
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let content_length: usize = String::from_utf8_lossy(&buf[..header_end])
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .starts_with("content-length:")
                .then(|| l.split(':').nth(1)?.trim().parse().ok())
                .flatten()
        })
        .unwrap_or(0);
    while buf.len() < header_end + content_length {
        let n = stream.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    String::from_utf8_lossy(&buf).into_owned()
}

// Returns the base URL and the server thread's handle. Callers must make
// their client request *before* joining — joining first would deadlock,
// since the spawned thread blocks in `accept()` until that request arrives.
fn serve_once(hub: FakeHub) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_full_request(&mut stream);
        let mut response = format!("HTTP/1.1 {} OK\r\nConnection: close\r\n", hub.status);
        for h in hub.headers {
            response.push_str(h);
            response.push_str("\r\n");
        }
        response.push_str(&format!("Content-Length: {}\r\n\r\n", hub.body.len()));
        stream.write_all(response.as_bytes()).unwrap();
        stream.write_all(hub.body).unwrap();
        request
    });
    (format!("http://{addr}"), handle)
}

fn client_for(addr: &str) -> HubClient {
    HubClient::new(addr, "my-repo").unwrap()
}

#[test]
fn pull_found_returns_the_body() {
    let (addr, handle) = serve_once(FakeHub {
        status: 200,
        headers: &[],
        body: b"snapshot-bytes",
    });
    let result = client_for(&addr).pull("abc123").unwrap();
    handle.join().unwrap();
    assert_eq!(result, PullOutcome::Found(b"snapshot-bytes".to_vec()));
}

#[test]
fn pull_404_maps_to_not_found() {
    let (addr, handle) = serve_once(FakeHub {
        status: 404,
        headers: &[],
        body: b"nope",
    });
    let result = client_for(&addr).pull("missing").unwrap();
    handle.join().unwrap();
    assert_eq!(result, PullOutcome::NotFound);
}

#[test]
fn push_201_publishes() {
    let (addr, handle) = serve_once(FakeHub {
        status: 201,
        headers: &[],
        body: b"",
    });
    let result = client_for(&addr)
        .push("target", Some("base"), 20, b"payload")
        .unwrap();
    handle.join().unwrap();
    assert_eq!(result, PushOutcome::Published);
}

#[test]
fn push_202_accepted() {
    let (addr, handle) = serve_once(FakeHub {
        status: 202,
        headers: &[],
        body: b"",
    });
    let result = client_for(&addr)
        .push("target", Some("base"), 20, b"payload")
        .unwrap();
    handle.join().unwrap();
    assert_eq!(result, PushOutcome::Accepted);
}

#[test]
fn push_409_conflict() {
    let (addr, handle) = serve_once(FakeHub {
        status: 409,
        headers: &[],
        body: b"",
    });
    let result = client_for(&addr)
        .push("target", Some("stale-base"), 20, b"payload")
        .unwrap();
    handle.join().unwrap();
    assert_eq!(result, PushOutcome::Conflict);
}

#[test]
fn push_429_surfaces_retry_after() {
    let (addr, handle) = serve_once(FakeHub {
        status: 429,
        headers: &["Retry-After: 42"],
        body: b"slow down",
    });
    let result = client_for(&addr)
        .push("target", None, 20, b"payload")
        .unwrap();
    handle.join().unwrap();
    assert_eq!(
        result,
        PushOutcome::RateLimited {
            retry_after_secs: Some(42)
        }
    );
}

#[test]
fn push_sends_repo_id_base_sha_and_retention_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_full_request(&mut stream);
        stream
            .write_all(b"HTTP/1.1 201 Created\r\nConnection: close\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        request
    });

    let client = HubClient::new(&format!("http://{addr}"), "r").unwrap();
    client.push("t", Some("b"), 7, b"payload").unwrap();
    let request = handle.join().unwrap();

    assert!(request.starts_with("PUT /snapshots/r/t.tar.zst HTTP/1.1"));
    assert!(request.contains("X-Weave-Repo-Id: r\r\n"));
    assert!(request.contains("X-Weave-Base-Sha: b\r\n"));
    assert!(request.contains("X-Weave-Retention: 7\r\n"));
    assert!(request.contains("Content-Length: 7\r\n"));
}

#[test]
fn https_urls_are_rejected_with_a_clear_error() {
    let err = HubClient::new("https://hub.internal", "r").unwrap_err();
    assert!(err.to_string().contains("http://"), "got: {err}");
}

#[test]
fn url_prefix_is_preserved() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 8192];
        let n = stream.read(&mut buf).unwrap_or(0);
        let request = String::from_utf8_lossy(&buf[..n]).into_owned();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 2\r\n\r\nhi")
            .unwrap();
        request
    });

    let client = HubClient::new(&format!("http://{addr}/weave"), "r").unwrap();
    let result = client.pull("abc").unwrap();
    assert_eq!(result, PullOutcome::Found(b"hi".to_vec()));
    let request = handle.join().unwrap();
    assert!(request.starts_with("GET /weave/snapshots/r/abc.tar.zst HTTP/1.1\r\n"));
}

#[test]
fn parse_url_defaults_to_port_80_and_handles_no_path() {
    let parsed = parse_url("http://hub.internal").unwrap();
    assert_eq!(parsed.host, "hub.internal");
    assert_eq!(parsed.port, 80);
    assert_eq!(parsed.prefix, "");
}

#[test]
fn parse_url_rejects_non_http_schemes_and_garbage() {
    assert!(parse_url("https://hub").is_err());
    assert!(parse_url("http://").is_err());
    assert!(parse_url("http://host:notaport").is_err());
}
