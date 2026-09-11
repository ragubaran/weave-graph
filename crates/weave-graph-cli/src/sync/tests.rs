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

#[test]
fn push_refuses_when_not_on_a_git_branch() {
    let repo = init_repo("feature");
    // Not a git repo at all → current_branch is None → refused before any
    // network call (merge-only publish, plan.md §2.3).
    let (addr, _request) = serve_with_status(201, b"");
    write_config(repo.path(), &addr);

    let err = cmd_sync_push(repo.path()).unwrap_err();
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

    cmd_sync_push(repo.path()).unwrap();

    let request = request.join().unwrap();
    assert!(request.starts_with("PUT /snapshots/"));
    assert!(request.contains("X-Weave-Retention: 5\r\n"));
    assert!(request.contains("Content-Length: "));
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

fn serve_with_status(status: u16, body: &[u8]) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let body = body.to_vec();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_full_request(&mut stream);
        let response = format!(
            "HTTP/1.1 {status} S\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.write_all(&body).unwrap();
        request
    });
    (format!("http://{addr}"), handle)
}

fn serve_snapshot(body: &[u8]) -> (String, std::thread::JoinHandle<String>) {
    serve_with_status(200, body)
}
