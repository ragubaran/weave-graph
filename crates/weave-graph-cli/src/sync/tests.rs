use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;

use super::{cmd_sync_pull, cmd_sync_push};

fn init_repo(name: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let weave_dir = dir.path().join(".weave");
    fs::create_dir_all(&weave_dir).unwrap();
    let active_db = weave_dir.join("graph.db");
    let source = dir.path().join(format!("{name}.rs"));
    fs::write(&source, "pub fn exported(x: u32) {}\n").unwrap();
    let files = vec![source];
    crate::index::full_reindex(dir.path(), &weave_dir, &active_db, &files).unwrap();
    dir
}

fn write_config(root: &Path, hub_url: &str) {
    fs::write(
        root.join(".weave").join("config.toml"),
        format!("[hub]\nurl = \"{hub_url}\"\nsnapshot_retention = 5\n"),
    )
    .unwrap();
}

#[test]
fn pull_hydrates_the_snapshot_through_the_rebuild_rename_path() {
    let repo = init_repo("pulled");
    let (addr, _request) = serve_snapshot(b"hydrated-db-bytes");
    write_config(repo.path(), &addr);

    cmd_sync_pull(repo.path(), Some("abc123"), false).unwrap();

    let db = fs::read(repo.path().join(".weave").join("graph.db")).unwrap();
    assert_eq!(db, b"hydrated-db-bytes");
    // Core Invariant 2: no leftover staging file after the atomic rename.
    assert!(!repo.path().join(".weave").join("graph.db.rebuild").exists());
}

/// HUB-02: a configured `[hub] token` must reach the wire as a real
/// bearer header — not just accepted by config parsing.
#[test]
fn pull_attaches_the_configured_hub_token_as_a_bearer_header() {
    let repo = init_repo("pulled");
    let (addr, request) = serve_snapshot(b"bytes");
    fs::write(
        repo.path().join(".weave").join("config.toml"),
        format!("[hub]\nurl = \"{addr}\"\ntoken = \"s3cr3t\"\n"),
    )
    .unwrap();

    cmd_sync_pull(repo.path(), Some("abc123"), false).unwrap();

    let request = request.join().unwrap();
    assert!(request.contains("Authorization: Bearer s3cr3t\r\n"));
}

#[test]
fn pull_without_a_snapshot_reports_and_leaves_the_db_untouched() {
    let repo = init_repo("noop");
    let (addr, _request) = serve_with_status(404, b"");
    write_config(repo.path(), &addr);
    let before = fs::read(repo.path().join(".weave").join("graph.db")).unwrap();

    cmd_sync_pull(repo.path(), Some("abc"), false).unwrap();

    let db = fs::read(repo.path().join(".weave").join("graph.db")).unwrap();
    assert_eq!(db, before, "a 404 must never touch the active database");
}

#[test]
fn pull_requires_a_configured_hub_url() {
    let repo = init_repo("nohub");
    let err = cmd_sync_pull(repo.path(), Some("abc"), false).unwrap_err();
    assert!(err.to_string().contains("[hub] url"), "got: {err}");
}

/// The registry's returned signature used to be discarded straight into
/// `_signature`. `report_signature` now handles it — this pins that a
/// present signature doesn't make the pull
/// error or panic (this crate still verifies nothing; it just no longer
/// silently drops what the registry sent).
#[test]
fn pull_with_a_signature_header_succeeds_without_discarding_it() {
    let repo = init_repo("signed_pull");
    let (addr, _requests) = serve_sequence(vec![(200, "X-Weave-Signature: abc123\r\n")]);
    write_config(repo.path(), &addr);

    cmd_sync_pull(repo.path(), Some("abc123"), false).unwrap();
}

#[test]
fn push_refuses_when_not_on_a_git_branch() {
    let repo = init_repo("feature");
    // Not a git repo at all → current_branch is None → refused before any
    // network call (merge-only publish).
    let (addr, _request) = serve_with_status(201, b"");
    write_config(repo.path(), &addr);

    let err = cmd_sync_push(repo.path(), None).unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("default branch") || message.contains("git branch"),
        "got: {message}"
    );
}

#[test]
fn push_on_main_publishes_the_snapshot() {
    let repo = init_repo("publisher");
    let (addr, request) = serve_with_status(201, b"");
    write_config(repo.path(), &addr);
    let git_ok = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(args)
            .output()
            .unwrap()
            .status
            .success()
    };
    assert!(git_ok(&["init", "-b", "main"]));
    assert!(git_ok(&["add", "."]));
    assert!(git_ok(&[
        "-c",
        "user.email=t@t",
        "-c",
        "user.name=t",
        "commit",
        "-m",
        "x"
    ]));

    cmd_sync_push(repo.path(), None).unwrap();

    let request = request.join().unwrap();
    assert!(request.starts_with("PUT /snapshots/"));
    assert!(request.contains("X-Weave-Retention: 5\r\n"));
    assert!(request.contains("Content-Length: "));
    assert!(
        !request.to_ascii_lowercase().contains("x-weave-signature:"),
        "no signature was supplied — the header must not appear at all: {request}"
    );
}

/// `cmd_sync_push`'s `signature` parameter is a real, honest seam, not a
/// no-op — a caller that supplies one gets it on the wire as
/// `X-Weave-Signature`, unchanged from before this refactor except that
/// it's no longer hardcoded to `None` three calls deep.
#[test]
fn push_with_a_signature_sends_the_x_weave_signature_header() {
    let repo = init_repo("signed");
    let (addr, request) = serve_with_status(201, b"");
    write_config(repo.path(), &addr);
    let git_ok = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(args)
            .output()
            .unwrap()
            .status
            .success()
    };
    assert!(git_ok(&["init", "-b", "main"]));
    assert!(git_ok(&["add", "."]));
    assert!(git_ok(&[
        "-c",
        "user.email=t@t",
        "-c",
        "user.name=t",
        "commit",
        "-m",
        "x"
    ]));

    cmd_sync_push(repo.path(), Some("deadbeef")).unwrap();

    let request = request.join().unwrap();
    assert!(
        request.contains("X-Weave-Signature: deadbeef\r\n"),
        "got: {request}"
    );
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

/// A real push (any non-empty payload, every fixture here) is preceded by
/// an unconditional `HEAD` probe — `HubClient::push` resumes from
/// `Upload-Offset` if the hub reports one, and starts a fresh chunk loop
/// at 0 otherwise. `404` here means exactly that: no upload in flight yet.
fn respond_404_to_head(stream: &mut std::net::TcpStream) {
    stream
        .write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Length: 0\r\n\r\n")
        .unwrap();
}

fn serve_with_status(status: u16, body: &[u8]) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let body = body.to_vec();
    let handle = std::thread::spawn(move || {
        loop {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_full_request(&mut stream);
            if request.starts_with("HEAD") {
                respond_404_to_head(&mut stream);
                continue;
            }
            let response = format!(
                "HTTP/1.1 {status} S\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.write_all(&body).unwrap();
            return request;
        }
    });
    (format!("http://{addr}"), handle)
}

fn serve_snapshot(body: &[u8]) -> (String, std::thread::JoinHandle<String>) {
    serve_with_status(200, body)
}

/// Serves each `(status, headers)` pair in order, one per accepted PUT —
/// for exercising `push_with_backoff`'s retry loop, where the client makes
/// more than one request against the same address. Each PUT is preceded by
/// its own `HEAD` probe (`HubClient::push` starts fresh every call, per
/// `serve_with_status`'s own comment) — drained and answered `404` without
/// consuming a queued `(status, headers)` entry.
fn serve_sequence(
    responses: Vec<(u16, &'static str)>,
) -> (String, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, extra_headers) in responses {
            let request = loop {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_full_request(&mut stream);
                if request.starts_with("HEAD") {
                    respond_404_to_head(&mut stream);
                    continue;
                }
                let response = format!(
                    "HTTP/1.1 {status} S\r\nConnection: close\r\n{extra_headers}Content-Length: 0\r\n\r\n"
                );
                stream.write_all(response.as_bytes()).unwrap();
                break request;
            };
            requests.push(request);
        }
        requests
    });
    (format!("http://{addr}"), handle)
}

#[test]
fn push_retries_with_backoff_after_a_rate_limit_and_then_succeeds() {
    let repo = init_repo("ratelimited");
    let (addr, requests) = serve_sequence(vec![(429, "Retry-After: 0\r\n"), (201, "")]);
    write_config(repo.path(), &addr);
    let git_ok = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(args)
            .output()
            .unwrap()
            .status
            .success()
    };
    assert!(git_ok(&["init", "-b", "main"]));
    assert!(git_ok(&["add", "."]));
    assert!(git_ok(&[
        "-c",
        "user.email=t@t",
        "-c",
        "user.name=t",
        "commit",
        "-m",
        "x"
    ]));

    cmd_sync_push(repo.path(), None).unwrap();

    assert_eq!(
        requests.join().unwrap().len(),
        2,
        "expected exactly one retry"
    );
}

#[test]
fn push_retries_past_a_second_conflict_before_giving_up() {
    let repo = init_repo("racy");
    // Two 409s (the hub head kept moving) then success — pins that a
    // conflict republish loops rather than giving up after one retry.
    let (addr, requests) = serve_sequence(vec![(409, ""), (409, ""), (201, "")]);
    write_config(repo.path(), &addr);
    let git_ok = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(args)
            .output()
            .unwrap()
            .status
            .success()
    };
    assert!(git_ok(&["init", "-b", "main"]));
    assert!(git_ok(&["add", "."]));
    assert!(git_ok(&[
        "-c",
        "user.email=t@t",
        "-c",
        "user.name=t",
        "commit",
        "-m",
        "x"
    ]));

    cmd_sync_push(repo.path(), None).unwrap();

    assert_eq!(
        requests.join().unwrap().len(),
        3,
        "expected the initial push plus two conflict retries"
    );
}

#[test]
fn push_gives_up_after_exhausting_conflict_retries() {
    let repo = init_repo("permastale");
    // Every attempt conflicts — the loop must bail with a clear error
    // instead of retrying forever.
    let (addr, requests) = serve_sequence(vec![(409, ""), (409, ""), (409, ""), (409, "")]);
    write_config(repo.path(), &addr);
    let git_ok = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(args)
            .output()
            .unwrap()
            .status
            .success()
    };
    assert!(git_ok(&["init", "-b", "main"]));
    assert!(git_ok(&["add", "."]));
    assert!(git_ok(&[
        "-c",
        "user.email=t@t",
        "-c",
        "user.name=t",
        "commit",
        "-m",
        "x"
    ]));

    let err = cmd_sync_push(repo.path(), None).unwrap_err();

    assert!(err.to_string().contains("republish attempts"), "got: {err}");
    assert_eq!(
        requests.join().unwrap().len(),
        4,
        "expected the initial push plus 3 exhausted conflict retries"
    );
}
