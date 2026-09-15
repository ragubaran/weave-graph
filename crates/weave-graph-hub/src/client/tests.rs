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
        let mut final_request = String::new();
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let request = read_full_request(&mut stream);
            if request.starts_with("HEAD ") {
                let response = "HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n";
                stream.write_all(response.as_bytes()).unwrap();
                continue;
            }
            let mut response = format!("HTTP/1.1 {} OK\r\nConnection: close\r\n", hub.status);
            for h in hub.headers {
                response.push_str(h);
                response.push_str("\r\n");
            }
            response.push_str(&format!("Content-Length: {}\r\n\r\n", hub.body.len()));
            stream.write_all(response.as_bytes()).unwrap();
            stream.write_all(hub.body).unwrap();
            final_request = request;
            break;
        }
        final_request
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
    assert_eq!(result, PullOutcome::Found(b"snapshot-bytes".to_vec(), None));
}

/// HUB-02: `with_token` must attach the bearer header to every request,
/// including `pull` (which sends no other caller-supplied headers at all).
#[test]
fn with_token_sends_an_authorization_bearer_header() {
    let (addr, handle) = serve_once(FakeHub {
        status: 200,
        headers: &[],
        body: b"bytes",
    });
    let client = HubClient::new(&addr, "my-repo")
        .unwrap()
        .with_token("s3cr3t");
    client.pull("abc123").unwrap();
    let request = handle.join().unwrap();
    assert!(request.contains("Authorization: Bearer s3cr3t\r\n"));
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
        .push("target", Some("base"), 20, None, b"payload")
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
        .push("target", Some("base"), 20, None, b"payload")
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
        .push("target", Some("stale-base"), 20, None, b"payload")
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
        .push("target", None, 20, None, b"payload")
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
        let mut final_req = String::new();
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let request = read_full_request(&mut stream);
            if request.starts_with("HEAD ") {
                let response = "HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n";
                stream.write_all(response.as_bytes()).unwrap();
                continue;
            }
            stream
                .write_all(
                    b"HTTP/1.1 201 Created\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
                )
                .unwrap();
            final_req = request;
            break;
        }
        final_req
    });

    let client = HubClient::new(&format!("http://{addr}"), "r").unwrap();
    client.push("t", Some("b"), 7, None, b"payload").unwrap();
    let request = handle.join().unwrap();

    assert!(request.starts_with("PUT /snapshots/r/t.tar.zst HTTP/1.1"));
    assert!(request.contains("X-Weave-Repo-Id: r\r\n"));
    assert!(request.contains("X-Weave-Base-Sha: b\r\n"));
    assert!(request.contains("X-Weave-Retention: 7\r\n"));
    assert!(request.contains("Content-Length: 7\r\n"));
    assert!(request.contains("Content-Range: bytes 0-6/7\r\n"));
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
    assert_eq!(result, PullOutcome::Found(b"hi".to_vec(), None));
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
    assert!(parse_url("ftp://hub").is_err());
    assert!(parse_url("http://").is_err());
    assert!(parse_url("http://host:notaport").is_err());
}

#[test]
fn base_headers_include_optional_snapshot_metadata() {
    let headers = client_for("http://127.0.0.1:9").base_headers(7, Some("base"), Some("sig"));
    assert!(headers.contains(&("X-Weave-Repo-Id".to_string(), "my-repo".to_string())));
    assert!(headers.contains(&("X-Weave-Retention".to_string(), "7".to_string())));
    assert!(headers.contains(&("X-Weave-Base-Sha".to_string(), "base".to_string())));
    assert!(headers.contains(&("X-Weave-Signature".to_string(), "sig".to_string())));
}

#[test]
fn map_status_covers_all_protocol_outcomes() {
    let client = client_for("http://127.0.0.1:9");
    assert_eq!(client.map_status(200, "").unwrap(), PushOutcome::Published);
    assert_eq!(client.map_status(202, "").unwrap(), PushOutcome::Accepted);
    assert_eq!(client.map_status(409, "").unwrap(), PushOutcome::Conflict);
    assert_eq!(
        client.map_status(429, "Retry-After: 12\r\n").unwrap(),
        PushOutcome::RateLimited {
            retry_after_secs: Some(12)
        }
    );
    assert_eq!(
        client.map_status(429, "Retry-After: invalid\r\n").unwrap(),
        PushOutcome::RateLimited {
            retry_after_secs: None
        }
    );
    let error = client.map_status(500, "").unwrap_err();
    assert!(error.to_string().contains("unexpected status 500"));
}

#[test]
fn push_chunks_rejects_an_incomplete_source_chunk() {
    let error = client_for("http://127.0.0.1:9")
        .push_chunks("target", None, 1, None, 3, |_offset, _length| {
            Ok(vec![1, 2])
        })
        .unwrap_err();
    assert!(error.to_string().contains("incomplete chunk"));
}

#[test]
fn parse_url_parses_an_explicit_port_and_prefix() {
    let parsed = parse_url("http://hub.internal:9000/weave").unwrap();
    assert_eq!(parsed.host, "hub.internal");
    assert_eq!(parsed.port, 9000);
    assert_eq!(parsed.prefix, "/weave");
}

#[test]
fn parse_response_rejects_garbage_in_every_branch() {
    // No `\r\n\r\n` separator at all.
    let err = parse_response(b"HTTP/1.1 200 OK no separator").unwrap_err();
    assert!(err.to_string().contains("separator"), "got: {err}");

    // Non-UTF-8 header bytes.
    let err = parse_response(b"\xff\xfe\xfd\r\n\r\nbody").unwrap_err();
    assert!(err.to_string().contains("non-utf8"), "got: {err}");

    // Separated, but no parseable status line.
    let err = parse_response(b"garbage headers\r\n\r\nbody").unwrap_err();
    assert!(err.to_string().contains("status"), "got: {err}");
}

#[test]
fn pull_surfaces_the_signature_header_when_present() {
    let (addr, handle) = serve_once(FakeHub {
        status: 200,
        headers: &["X-Weave-Signature: deadbeef"],
        body: b"bytes",
    });
    let result = client_for(&addr).pull("abc").unwrap();
    handle.join().unwrap();
    assert_eq!(
        result,
        PullOutcome::Found(b"bytes".to_vec(), Some("deadbeef".to_string()))
    );
}

#[test]
fn pull_unexpected_status_is_a_malformed_response_error() {
    let (addr, handle) = serve_once(FakeHub {
        status: 500,
        headers: &[],
        body: b"boom",
    });
    let err = client_for(&addr).pull("abc").unwrap_err();
    handle.join().unwrap();
    assert!(
        err.to_string().contains("unexpected status 500"),
        "got: {err}"
    );
}

#[test]
fn push_unexpected_status_is_a_malformed_response_error() {
    let (addr, handle) = serve_once(FakeHub {
        status: 500,
        headers: &[],
        body: b"",
    });
    let err = client_for(&addr)
        .push("t", None, 1, None, b"payload")
        .unwrap_err();
    handle.join().unwrap();
    assert!(
        err.to_string().contains("unexpected status 500"),
        "got: {err}"
    );
}

#[test]
fn push_empty_payload_takes_the_zero_length_path() {
    let (addr, handle) = serve_once(FakeHub {
        status: 204,
        headers: &[],
        body: b"",
    });
    let result = client_for(&addr).push("t", None, 1, None, b"").unwrap();
    let request = handle.join().unwrap();
    assert_eq!(result, PushOutcome::Published);
    assert!(
        request.contains("Content-Range: bytes 0-0/0\r\n"),
        "the zero-length envelope: {request}"
    );
}

#[test]
fn push_file_publishes_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let snapshot = dir.path().join("snap.tar.zst");
    std::fs::write(&snapshot, b"file-bytes").unwrap();

    let (addr, handle) = serve_once(FakeHub {
        status: 201,
        headers: &[],
        body: b"",
    });
    let result = client_for(&addr)
        .push_file("t", None, 1, None, &snapshot)
        .unwrap();
    handle.join().unwrap();
    assert_eq!(result, PushOutcome::Published);
}

#[test]
fn push_file_reports_a_missing_file_as_an_io_error() {
    let err = client_for("http://127.0.0.1:9")
        .push_file(
            "t",
            None,
            1,
            None,
            std::path::Path::new("/definitely/absent.bin"),
        )
        .unwrap_err();
    assert!(matches!(err, HubError::Io(_)), "got: {err:?}");
}

#[test]
fn push_resumes_from_the_server_reported_upload_offset() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut put_request = String::new();
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_full_request(&mut stream);
            if request.starts_with("HEAD ") {
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nUpload-Offset: 2\r\nConnection: close\r\n\r\n")
                    .unwrap();
            } else {
                stream
                    .write_all(b"HTTP/1.1 201 Created\r\nConnection: close\r\n\r\n")
                    .unwrap();
                put_request = request;
            }
        }
        put_request
    });

    let outcome = client_for(&format!("http://{address}"))
        .push("target", None, 1, None, b"abcd")
        .unwrap();
    assert_eq!(outcome, PushOutcome::Published);
    assert!(
        server
            .join()
            .unwrap()
            .contains("Content-Range: bytes 2-3/4\r\n")
    );
}

#[test]
fn push_returns_the_transport_error_after_retry_exhaustion() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..8 {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_full_request(&mut stream);
            if request.starts_with("HEAD ") {
                stream
                    .write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n")
                    .unwrap();
            }
        }
    });

    let error = client_for(&format!("http://{address}"))
        .push("target", None, 1, None, b"payload")
        .unwrap_err();
    server.join().unwrap();
    assert!(error.to_string().contains("separator"));
}

#[test]
fn push_requeries_the_server_offset_after_a_transient_transport_error() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for request_number in 0..4 {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_full_request(&mut stream);
            match request_number {
                0 => stream
                    .write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n")
                    .unwrap(),
                2 => stream
                    .write_all(b"HTTP/1.1 200 OK\r\nUpload-Offset: 0\r\nConnection: close\r\n\r\n")
                    .unwrap(),
                3 => stream
                    .write_all(b"HTTP/1.1 201 Created\r\nConnection: close\r\n\r\n")
                    .unwrap(),
                _ => assert!(request.starts_with("PUT ")),
            }
        }
    });

    let outcome = client_for(&format!("http://{address}"))
        .push("target", None, 1, None, b"payload")
        .unwrap();
    server.join().unwrap();
    assert_eq!(outcome, PushOutcome::Published);
}
