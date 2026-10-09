use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;

use super::{cmd_sync_pull, cmd_sync_push};

#[cfg(feature = "vector")]
use weave_graph_core::embedding::MockEmbeddingProvider;
#[cfg(feature = "vector")]
use weave_graph_core::{Node, Storage};
#[cfg(feature = "vector")]
use weave_graph_store_sqlite::SqliteStorage;

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

#[cfg(feature = "vector")]
#[test]
fn push_snapshot_excludes_configured_vector_paths() {
    let repo = init_repo("vector_snapshot");
    let db_path = repo.path().join(".weave/graph.db");
    let mut storage = SqliteStorage::open(&db_path).unwrap();
    let secret_id = storage
        .upsert_node(&Node {
            id: 0,
            repo_id: "local".to_string(),
            path: "private/secret.rs".to_string(),
            symbol: "secret".to_string(),
            kind: "function".to_string(),
            line_start: 1,
            line_end: 1,
            signature: "fn secret()".to_string(),
        })
        .unwrap();
    let public_id = storage
        .upsert_node(&Node {
            id: 0,
            repo_id: "local".to_string(),
            path: "public/api.rs".to_string(),
            symbol: "public_api".to_string(),
            kind: "function".to_string(),
            line_start: 1,
            line_end: 1,
            signature: "fn public_api()".to_string(),
        })
        .unwrap();
    let embedder = MockEmbeddingProvider::new();
    storage
        .rebuild_vector_index(
            &embedder,
            &[
                (secret_id, "private secret credential".to_string()),
                (public_id, "public api credential".to_string()),
            ],
        )
        .unwrap();
    drop(storage);
    fs::write(
        repo.path().join(".weave/config.toml"),
        "[vector]\nexclude = [\"private\"]\n",
    )
    .unwrap();

    let snapshot = super::snapshot_for_push(repo.path(), &db_path).unwrap();
    let storage = SqliteStorage::open(&snapshot.path).unwrap();
    let hits = storage
        .search_vector(&embedder, "credential", 5, 4, None)
        .unwrap();

    assert!(!hits.contains(&secret_id));
    assert!(hits.contains(&public_id));
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

/// No `--commit` given: the sha is resolved via `git merge-base
/// origin/main HEAD`, not left for the hub to guess.
#[test]
fn pull_resolves_the_commit_via_git_merge_base_when_none_is_given() {
    let repo = init_repo("mergebase");
    let (addr, _request) = serve_snapshot(b"merge-base-bytes");
    write_config(repo.path(), &addr);
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(args)
            .output()
            .unwrap()
    };
    assert!(git(&["init", "-q"]).status.success());
    assert!(git(&["add", "-A"]).status.success());
    assert!(
        git(&[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "x"
        ])
        .status
        .success()
    );
    let head = String::from_utf8(git(&["rev-parse", "HEAD"]).stdout)
        .unwrap()
        .trim()
        .to_string();
    assert!(
        git(&["update-ref", "refs/remotes/origin/main", &head])
            .status
            .success()
    );

    cmd_sync_pull(repo.path(), None, false).unwrap();

    let db = fs::read(repo.path().join(".weave").join("graph.db")).unwrap();
    assert_eq!(db, b"merge-base-bytes");
}

/// A shallow checkout (or a repo with no commits at all) has no
/// merge-base to compute — the error must name the actual fix, not
/// surface git's own raw failure.
#[test]
fn pull_reports_a_clear_error_when_merge_base_cannot_be_determined() {
    let repo = init_repo("shallow");
    // Unreachable address — the failure must come from git, before any
    // network attempt.
    write_config(repo.path(), "http://127.0.0.1:1");
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
    assert!(git_ok(&["init", "-q"]));

    let err = cmd_sync_pull(repo.path(), None, false).unwrap_err();
    assert!(err.to_string().contains("merge-base"), "got: {err}");
}

/// `--fallback-latest`: an exact-sha miss transparently hydrates the
/// hub's latest snapshot instead of reporting nothing.
#[test]
fn pull_fallback_latest_hydrates_when_the_exact_commit_is_missing() {
    let repo = init_repo("fallback");
    let (addr, requests) = serve_sequence(vec![(404, ""), (200, "")]);
    write_config(repo.path(), &addr);

    cmd_sync_pull(repo.path(), Some("missing-sha"), true).unwrap();

    assert_eq!(requests.join().unwrap().len(), 2, "exact sha then latest");
}

/// `--fallback-latest` with nothing on the hub at all for either the
/// exact sha or latest: reported, never an error, and the active db is
/// left untouched.
#[test]
fn pull_fallback_latest_still_reports_when_no_snapshot_exists_at_all() {
    let repo = init_repo("none_anywhere");
    let (addr, requests) = serve_sequence(vec![(404, ""), (404, "")]);
    write_config(repo.path(), &addr);
    let before = fs::read(repo.path().join(".weave").join("graph.db")).unwrap();

    cmd_sync_pull(repo.path(), Some("missing-sha"), true).unwrap();

    let db = fs::read(repo.path().join(".weave").join("graph.db")).unwrap();
    assert_eq!(
        db, before,
        "no snapshot anywhere must never touch the active database"
    );
    assert_eq!(requests.join().unwrap().len(), 2);
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

    let err = cmd_sync_push(repo.path(), None, None, "hmac").unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("default branch") || message.contains("git branch"),
        "got: {message}"
    );
}

/// A real (non-detached) branch that isn't `main`/`master` is refused
/// too — distinct from the no-git-repo-at-all case above, which never
/// resolves a branch name in the first place.
#[test]
fn push_refuses_from_a_non_default_branch() {
    let repo = init_repo("sidebranch");
    let (addr, _request) = serve_with_status(201, b"");
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
    assert!(git_ok(&["init", "-b", "feature-x"]));
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

    let err = cmd_sync_push(repo.path(), None, None, "hmac").unwrap_err();
    assert!(err.to_string().contains("feature-x"), "got: {err}");
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

    cmd_sync_push(repo.path(), None, None, "hmac").unwrap();

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

    cmd_sync_push(repo.path(), Some("deadbeef"), None, "hmac").unwrap();

    let request = request.join().unwrap();
    assert!(
        request.contains("X-Weave-Signature: deadbeef\r\n"),
        "got: {request}"
    );
}

#[cfg(feature = "hub-provenance")]
#[test]
fn push_signature_can_be_computed_from_a_local_secret_file() {
    use weave_graph_hub::{HmacSnapshotProvenanceVerifier, SnapshotProvenanceVerifier};

    let dir = tempfile::tempdir().unwrap();
    let key_path = dir.path().join("provenance.key");
    let snapshot_path = dir.path().join("snapshot.db");
    fs::write(&key_path, [3u8; 32]).unwrap();
    fs::write(&snapshot_path, b"snapshot bytes").unwrap();

    let signature = super::push_signature(
        dir.path(),
        "abc123",
        &snapshot_path,
        None,
        Some(&key_path),
        "hmac",
    )
    .unwrap()
    .unwrap();
    let expected = HmacSnapshotProvenanceVerifier::new([3u8; 32])
        .unwrap()
        .sign_snapshot(&super::repo_label(dir.path()), "abc123", b"snapshot bytes")
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    assert_eq!(signature, expected);
}

#[cfg(feature = "hub-provenance")]
#[test]
fn push_signature_can_be_computed_with_the_ed25519_provider() {
    use weave_graph_hub::{Ed25519SnapshotProvenanceVerifier, SnapshotProvenanceVerifier};

    let dir = tempfile::tempdir().unwrap();
    let key_path = dir.path().join("provenance.key");
    let snapshot_path = dir.path().join("snapshot.db");
    fs::write(&key_path, [3u8; 32]).unwrap();
    fs::write(&snapshot_path, b"snapshot bytes").unwrap();

    let signature = super::push_signature(
        dir.path(),
        "abc123",
        &snapshot_path,
        None,
        Some(&key_path),
        "ed25519",
    )
    .unwrap()
    .unwrap();
    let expected = Ed25519SnapshotProvenanceVerifier::new([3u8; 32])
        .unwrap()
        .sign_snapshot(&super::repo_label(dir.path()), "abc123", b"snapshot bytes")
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    assert_eq!(signature, expected);
    // Ed25519 signatures are 64 bytes (128 hex chars); HMAC-SHA-256's are 32 (64 hex chars) —
    // confirms this actually used a different scheme, not just a coincidentally-equal path.
    assert_eq!(signature.len(), 128);
}

#[cfg(feature = "hub-provenance")]
#[test]
fn push_signature_rejects_an_unknown_provenance_provider() {
    let dir = tempfile::tempdir().unwrap();
    let key_path = dir.path().join("provenance.key");
    let snapshot_path = dir.path().join("snapshot.db");
    fs::write(&key_path, [3u8; 32]).unwrap();
    fs::write(&snapshot_path, b"snapshot bytes").unwrap();

    let error = super::push_signature(
        dir.path(),
        "abc123",
        &snapshot_path,
        None,
        Some(&key_path),
        "rsa",
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("unknown provenance provider"), "{error}");
}

#[test]
fn push_signature_rejects_two_signing_sources() {
    let dir = tempfile::tempdir().unwrap();
    let error = super::push_signature(
        dir.path(),
        "abc123",
        &dir.path().join("snapshot.db"),
        Some("deadbeef"),
        Some(&dir.path().join("key")),
        "hmac",
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("either --signature or --provenance-key-file"));
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

    cmd_sync_push(repo.path(), None, None, "hmac").unwrap();

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

    cmd_sync_push(repo.path(), None, None, "hmac").unwrap();

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

    let err = cmd_sync_push(repo.path(), None, None, "hmac").unwrap_err();

    assert!(err.to_string().contains("republish attempts"), "got: {err}");
    assert_eq!(
        requests.join().unwrap().len(),
        4,
        "expected the initial push plus 3 exhausted conflict retries"
    );
}

/// The entire `MAX_PUSH_ATTEMPTS` retry budget is spent rate-limited —
/// `push_with_backoff` gives up cleanly rather than looping forever, and
/// `cmd_sync_push` reports it through the top-level `RateLimited` arm
/// (not the conflict-retry one below).
#[test]
fn push_errors_clearly_when_rate_limited_for_the_entire_retry_budget() {
    let repo = init_repo("limited_forever");
    let responses: Vec<(u16, &'static str)> =
        std::iter::repeat_n((429, "Retry-After: 0\r\n"), 5).collect();
    let (addr, requests) = serve_sequence(responses);
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

    let err = cmd_sync_push(repo.path(), None, None, "hmac").unwrap_err();

    assert!(
        err.to_string().contains("still limited after"),
        "got: {err}"
    );
    assert_eq!(requests.join().unwrap().len(), 5);
}

/// A conflict republish that then runs into a rate limit surfaces through
/// the conflict-retry loop's own `RateLimited` arm — distinct error text
/// from the top-level one above.
#[test]
fn push_conflict_retry_errors_clearly_if_the_hub_then_rate_limits() {
    let repo = init_repo("conflict_then_limited");
    let mut responses = vec![(409, "")];
    responses.extend(std::iter::repeat_n((429, "Retry-After: 0\r\n"), 5));
    let (addr, requests) = serve_sequence(responses);
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

    let err = cmd_sync_push(repo.path(), None, None, "hmac").unwrap_err();

    assert!(
        err.to_string()
            .contains("rate-limited the publish during conflict"),
        "got: {err}"
    );
    assert_eq!(requests.join().unwrap().len(), 6);
}
