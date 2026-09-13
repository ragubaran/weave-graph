use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener};

use super::*;

/// A one-shot listener that captures the raw request text and replies
/// `200 OK` — enough to prove [`deliver`] sends the right method/path/body
/// without needing a real HTTP server dependency.
fn capture_one_request() -> (SocketAddr, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = stream.read(&mut chunk).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        String::from_utf8_lossy(&buf).into_owned()
    });
    (addr, handle)
}

/// `deliver` takes an already-validated [`SocketAddr`], not a hostname —
/// this is the wire-format test, run against a loopback listener directly
/// (the only reachable target in a sandboxed test run). The SSRF guard
/// itself, which `notify`/`validate_registerable` enforce *before*
/// `deliver` ever runs, is covered by its own tests below.
#[test]
fn deliver_posts_the_repo_id_and_commit_sha_as_json() {
    let (addr, handle) = capture_one_request();
    deliver(addr, &addr.to_string(), "/incoming", "my-repo", "abc123").unwrap();
    let request = handle.join().unwrap();

    assert!(request.starts_with("POST /incoming HTTP/1.1"), "{request}");
    assert!(
        request.contains("Content-Type: application/json"),
        "{request}"
    );
    assert!(
        request.contains(r#"{"repo_id":"my-repo","commit_sha":"abc123"}"#),
        "{request}"
    );
}

#[test]
fn deliver_reports_an_unreachable_address_as_an_error_not_a_panic() {
    // Port 0 as a *destination* is invalid but resolvable — the OS refuses
    // the connect immediately, no real network wait. `deliver` enforces no
    // SSRF policy itself (that's `notify`'s job upstream of it), so it's
    // fine to point this directly at loopback.
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let err = deliver(addr, "127.0.0.1", "/hook", "r", "sha").unwrap_err();
    assert!(err.contains("connect"), "{err}");
}

#[test]
fn notify_rejects_a_non_http_scheme_without_connecting() {
    let err = notify("https://example.com/hook", "r", "sha").unwrap_err();
    assert!(err.contains("http://"), "{err}");
}

#[test]
fn parse_webhook_url_rejects_userinfo() {
    let err = parse_webhook_url("http://user:pass@evil.example/hook").unwrap_err();
    assert!(err.contains("userinfo"), "{err}");
}

/// The SSRF guard: a registry-side webhook target must never resolve to
/// loopback, RFC1918/link-local, or the cloud-metadata address — `notify`
/// (the real dispatch path) and `validate_registerable` (the registration
/// path) both reject these before ever attempting a connection.
#[test]
fn notify_rejects_loopback_private_and_link_local_targets_without_connecting() {
    // IPv6 literals (`http://[::1]/...`) aren't parsed by `parse_webhook_url`
    // at all yet (pre-existing v1 scope limit, not this guard's concern) —
    // `is_blocked_addr_classifies_known_address_ranges` covers IPv6
    // classification directly instead.
    for url in [
        "http://127.0.0.1:9/hook",
        "http://10.1.2.3/hook",
        "http://192.168.1.1/hook",
        "http://169.254.169.254/hook",
    ] {
        let err = notify(url, "r", "sha").unwrap_err();
        assert!(err.contains("non-public"), "{url} -> {err}");
    }
}

#[test]
fn validate_registerable_rejects_a_loopback_target() {
    let err = validate_registerable("http://127.0.0.1:9/hook").unwrap_err();
    assert!(err.contains("non-public"), "{err}");
}

#[test]
fn validate_registerable_accepts_a_public_ip_literal() {
    // Never actually dialed — resolving an IP literal needs no DNS lookup,
    // so this stays fast and network-independent.
    validate_registerable("http://8.8.8.8/hook").unwrap();
}

#[test]
fn is_blocked_addr_classifies_known_address_ranges() {
    let blocked = [
        "127.0.0.1",
        "10.0.0.1",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.169.254",
        "0.0.0.0",
        "255.255.255.255",
        "::1",
        "::",
        "fe80::1",
        "fc00::1",
    ];
    for ip in blocked {
        let addr: IpAddr = ip.parse().unwrap();
        assert!(is_blocked_addr(&addr), "{ip} should be blocked");
    }

    let public = ["8.8.8.8", "1.1.1.1", "2001:4860:4860::8888"];
    for ip in public {
        let addr: IpAddr = ip.parse().unwrap();
        assert!(!is_blocked_addr(&addr), "{ip} should not be blocked");
    }
}
