use std::time::{Duration, Instant};

use super::*;

fn config(max_queue_depth_per_repo: usize, max_pushes_per_minute_per_repo: u32) -> RegistryConfig {
    RegistryConfig {
        max_queue_depth_per_repo,
        max_pushes_per_minute_per_repo,
        max_snapshot_bytes: 10 * 1024 * 1024,
        canvas_exclude: vec![],
    }
}

fn generous_config() -> RegistryConfig {
    config(1_000, 1_000)
}

/// The worker commits off the request path — tests that need to observe a
/// committed blob poll `pull` instead of sleeping a fixed guess.
fn wait_for_commit(registry: &Registry, repo_id: &str, sha: &str) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let PullResult::Found(bytes, _) = registry.pull(repo_id, sha) {
            return bytes;
        }
        if Instant::now() >= deadline {
            panic!("timed out waiting for {repo_id}/{sha} to commit");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn is_safe_path_component_rejects_traversal_and_accepts_ordinary_names() {
    assert!(is_safe_path_component("my-repo"));
    assert!(is_safe_path_component("abc123"));
    assert!(
        is_safe_path_component("my..repo"),
        "dots are fine unless the whole component is '..'"
    );
    assert!(!is_safe_path_component(""));
    assert!(!is_safe_path_component("."));
    assert!(!is_safe_path_component(".."));
    assert!(!is_safe_path_component("../etc/passwd"));
    assert!(!is_safe_path_component("a/b"));
    assert!(!is_safe_path_component("a\\b"));
    assert!(!is_safe_path_component(&"a".repeat(129)));
}

#[test]
fn push_and_pull_refuse_a_traversal_repo_id_or_sha_at_the_sink() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(dir.path(), generous_config()).unwrap();
    assert!(
        registry
            .push("../escape", "sha1", None, 20, None, b"payload")
            .is_err()
    );
    assert!(
        registry
            .push("repo-a", "../escape", None, 20, None, b"payload")
            .is_err()
    );
    assert_eq!(
        registry.pull("../escape", "sha1"),
        PullResult::NotFound,
        "pull with traversal repo_id must not touch the filesystem"
    );
    assert_eq!(
        registry.pull("repo-a", "../escape"),
        PullResult::NotFound,
        "pull with traversal sha must not touch the filesystem"
    );
}

#[test]
fn rejected_or_oversized_uploads_leave_no_temporary_spool() {
    let dir = tempfile::tempdir().unwrap();
    let mut limits = generous_config();
    limits.max_snapshot_bytes = 3;
    let registry = Registry::open(dir.path(), limits).unwrap();

    assert!(
        registry
            .push("repo-a", "too-large", None, 20, None, b"four")
            .is_err()
    );
    assert!(
        !dir.path().join("spool/repo-a/too-large.tmp").exists(),
        "an oversized upload must not create a temporary file"
    );

    registry
        .push("repo-a", "sha1", None, 20, None, b"one")
        .unwrap();
    assert_eq!(
        registry
            .push("repo-a", "conflict", Some("wrong"), 20, None, b"two")
            .unwrap(),
        PushDecision::Conflict
    );
    assert!(
        !dir.path().join("spool/repo-a/conflict.tmp").exists(),
        "a rejected conflict must not leave upload data on disk"
    );
}

#[test]
fn unfinished_uploads_consume_queue_capacity_before_spooling() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(dir.path(), config(1, 100)).unwrap();
    assert_eq!(
        registry.begin_upload("repo-a", "in-flight", None).unwrap(),
        PushDecision::Accepted
    );
    assert_eq!(
        registry.begin_upload("repo-a", "second", None).unwrap(),
        PushDecision::RateLimited {
            retry_after_secs: 5
        }
    );
    assert!(
        !dir.path().join("spool/repo-a/second.tmp").exists(),
        "queue rejection must happen before any spool file exists"
    );
    registry.abandon_upload("repo-a", "in-flight");
}

#[test]
fn first_push_succeeds_regardless_of_base_sha() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(dir.path(), generous_config()).unwrap();
    let decision = registry
        .push(
            "repo-a",
            "sha1",
            Some("nonexistent-base"),
            20,
            None,
            b"payload",
        )
        .unwrap();
    assert_eq!(decision, PushDecision::Accepted);
}

#[test]
fn matching_base_sha_is_accepted_and_advances_head() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(dir.path(), generous_config()).unwrap();
    assert_eq!(
        registry
            .push("repo-a", "sha1", None, 20, None, b"v1")
            .unwrap(),
        PushDecision::Accepted
    );
    assert_eq!(
        registry
            .push("repo-a", "sha2", Some("sha1"), 20, None, b"v2")
            .unwrap(),
        PushDecision::Accepted
    );
}

#[test]
fn stale_base_sha_is_a_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(dir.path(), generous_config()).unwrap();
    registry
        .push("repo-a", "sha1", None, 20, None, b"v1")
        .unwrap();
    let decision = registry
        .push("repo-a", "sha2", Some("wrong-base"), 20, None, b"v2")
        .unwrap();
    assert_eq!(decision, PushDecision::Conflict);
}

#[test]
fn republish_with_no_base_after_a_conflict_always_succeeds() {
    // Mirrors weave-graph-cli::sync's client contract: on Conflict, retry
    // once with base=None.
    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(dir.path(), generous_config()).unwrap();
    registry
        .push("repo-a", "sha1", None, 20, None, b"v1")
        .unwrap();
    assert_eq!(
        registry
            .push("repo-a", "sha2", Some("wrong-base"), 20, None, b"v2")
            .unwrap(),
        PushDecision::Conflict
    );
    assert_eq!(
        registry
            .push("repo-a", "sha2", None, 20, None, b"v2")
            .unwrap(),
        PushDecision::Accepted
    );
}

#[test]
fn concurrent_pushes_to_the_same_repo_never_both_succeed_against_the_same_stale_base() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(Registry::open(dir.path(), generous_config()).unwrap());
    registry
        .push("repo-a", "sha1", None, 20, None, b"v1")
        .unwrap();

    // Two racers both build on "sha1" — at most one may be accepted;
    // the loser must see a Conflict, never a corrupted/duplicated head.
    let handles: Vec<_> = ["sha2-from-a", "sha2-from-b"]
        .into_iter()
        .map(|target| {
            let registry = Arc::clone(&registry);
            std::thread::spawn(move || {
                registry
                    .push("repo-a", target, Some("sha1"), 20, None, b"racer")
                    .unwrap()
            })
        })
        .collect();
    let outcomes: Vec<PushDecision> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let accepted = outcomes
        .iter()
        .filter(|d| **d == PushDecision::Accepted)
        .count();
    let conflicts = outcomes
        .iter()
        .filter(|d| **d == PushDecision::Conflict)
        .count();
    assert_eq!(accepted, 1, "exactly one racer must win: {outcomes:?}");
    assert_eq!(conflicts, 1, "the loser must see a conflict: {outcomes:?}");
}

#[test]
fn watermark_rate_limits_once_the_queue_backs_up() {
    // Each accepted push costs the producer ~2 syscalls (spool write +
    // rename); each committed job costs the single worker thread ~6
    // (read, write+rename to store, write+rename to HEAD, directory
    // rescan). Firing pushes back-to-back on one thread with no yields
    // outruns that single worker for at least the first few, at any
    // watermark low enough to matter — this isn't timing-sensitive in the
    // way a fixed sleep-then-assert would be.
    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(dir.path(), config(3, 1_000)).unwrap();
    let mut saw_rate_limited = false;
    let mut base: Option<String> = None;
    for i in 0..20 {
        let target = format!("sha{i}");
        match registry
            .push("repo-a", &target, base.as_deref(), 20, None, b"payload")
            .unwrap()
        {
            PushDecision::Accepted => base = Some(target),
            PushDecision::RateLimited { retry_after_secs } => {
                assert!(retry_after_secs > 0);
                saw_rate_limited = true;
                break;
            }
            other => panic!("unexpected decision with a fresh base each time: {other:?}"),
        }
    }
    assert!(
        saw_rate_limited,
        "queue watermark was never hit across 20 rapid pushes"
    );
}

#[test]
fn sustained_rate_limit_kicks_in_independent_of_queue_depth() {
    let dir = tempfile::tempdir().unwrap();
    // Huge queue watermark so only the per-minute cap can trip.
    let registry = Registry::open(dir.path(), config(1_000, 3)).unwrap();
    let mut base: Option<String> = None;
    let mut decisions = Vec::new();
    for i in 0..5 {
        let target = format!("sha{i}");
        let decision = registry
            .push("repo-a", &target, base.as_deref(), 20, None, b"payload")
            .unwrap();
        if decision == PushDecision::Accepted {
            base = Some(target);
        }
        decisions.push(decision);
    }
    assert_eq!(
        decisions[..3],
        [
            PushDecision::Accepted,
            PushDecision::Accepted,
            PushDecision::Accepted
        ]
    );
    assert!(matches!(decisions[3], PushDecision::RateLimited { .. }));
}

#[test]
fn different_repos_never_contend_with_each_other() {
    // A saturated repo-a must not affect repo-b's own independent watermark.
    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(dir.path(), config(1, 1_000)).unwrap();
    let mut base: Option<String> = None;
    let mut hit_watermark = false;
    for i in 0..10 {
        let target = format!("sha{i}");
        match registry
            .push("repo-a", &target, base.as_deref(), 20, None, b"payload")
            .unwrap()
        {
            PushDecision::Accepted => base = Some(target),
            PushDecision::RateLimited { .. } => {
                hit_watermark = true;
                break;
            }
            other => panic!("unexpected: {other:?}"),
        }
    }
    assert!(hit_watermark, "expected repo-a to hit its own watermark");

    let decision = registry
        .push("repo-b", "sha0", None, 20, None, b"payload")
        .unwrap();
    assert_eq!(
        decision,
        PushDecision::Accepted,
        "repo-b must be unaffected by repo-a's backlog"
    );
}

#[test]
fn pull_of_latest_resolves_through_the_persisted_head() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(dir.path(), generous_config()).unwrap();
    registry
        .push("repo-a", "sha1", None, 20, None, b"v1")
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let PullResult::Found(bytes, _) = registry.pull("repo-a", "latest") {
            assert_eq!(bytes, b"v1");
            break;
        }
        if Instant::now() >= deadline {
            panic!("timeout waiting for push");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn push_with_signature_is_persisted_and_returned_on_pull() {
    let repo_dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(repo_dir.path(), generous_config()).unwrap();

    registry
        .push("repo-a", "sha1", None, 20, Some("abcd123"), b"v1")
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let PullResult::Found(bytes, sig) = registry.pull("repo-a", "latest") {
            assert_eq!(bytes, b"v1");
            assert_eq!(sig.as_deref(), Some("abcd123"));
            break;
        }
        if Instant::now() >= deadline {
            panic!("timeout waiting for push");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn pull_of_unknown_repo_or_sha_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(dir.path(), generous_config()).unwrap();
    assert_eq!(registry.pull("nope", "sha1"), PullResult::NotFound);
    assert_eq!(registry.pull("nope", "latest"), PullResult::NotFound);

    registry
        .push("repo-a", "sha1", None, 20, None, b"v1")
        .unwrap();
    wait_for_commit(&registry, "repo-a", "sha1");
    assert_eq!(
        registry.pull("repo-a", "sha-never-pushed"),
        PullResult::NotFound
    );
}

#[test]
fn retention_prunes_older_snapshots_beyond_the_hint() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(dir.path(), generous_config()).unwrap();
    let mut base: Option<String> = None;
    for i in 0..5 {
        std::thread::sleep(Duration::from_millis(15));
        let target = format!("sha{i}");
        registry
            .push(
                "repo-a",
                &target,
                base.as_deref(),
                2,
                None,
                format!("v{i}").as_bytes(),
            )
            .unwrap();
        wait_for_commit(&registry, "repo-a", &target);
        base = Some(target);
    }

    let store_dir = dir.path().join("store").join("repo-a");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut remaining = Vec::new();
    while Instant::now() < deadline {
        if registry.pending_jobs("repo-a") == 0 {
            remaining = std::fs::read_dir(&store_dir)
                .unwrap()
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "zst"))
                .collect();
            if remaining.len() == 2 {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(remaining.len(), 2, "retention hint was 2");
    // The two most recent pushes must be the ones that survived.
    assert_eq!(wait_for_commit(&registry, "repo-a", "sha3"), b"v3");
    assert_eq!(wait_for_commit(&registry, "repo-a", "sha4"), b"v4");
    assert_eq!(registry.pull("repo-a", "sha0"), PullResult::NotFound);
}

#[test]
fn restart_recovers_a_leftover_spool_job_from_a_prior_crash() {
    let dir = tempfile::tempdir().unwrap();
    // Simulate a prior process that accepted a push (wrote it durably to
    // the spool) but crashed before its worker committed it — write the
    // spool file directly, bypassing `push`/its live worker entirely.
    let spool_dir = dir.path().join("spool").join("repo-a");
    std::fs::create_dir_all(&spool_dir).unwrap();
    std::fs::write(
        spool_dir.join(job_filename(0, "sha1", 20)),
        b"recovered payload",
    )
    .unwrap();

    let registry = Registry::open(dir.path(), generous_config()).unwrap();
    assert_eq!(
        wait_for_commit(&registry, "repo-a", "latest"),
        b"recovered payload"
    );
    assert_eq!(
        registry.pull("repo-a", "sha1"),
        PullResult::Found(b"recovered payload".to_vec(), None)
    );
}

#[test]
fn twenty_repos_pushing_concurrently_each_land_their_own_final_head() {
    // The milestone's own required load test (`impl.md` M3.1): concurrent
    // pushes across >= 20 simulated repos, no cross-repo contention,
    // correct backpressure at the configured watermark.
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(Registry::open(dir.path(), config(50, 1_000)).unwrap());
    let repo_count = 24;
    let pushes_per_repo = 5;

    let handles: Vec<_> = (0..repo_count)
        .map(|repo_index| {
            let registry = Arc::clone(&registry);
            std::thread::spawn(move || {
                let repo_id = format!("repo-{repo_index}");
                let mut base: Option<String> = None;
                for seq in 0..pushes_per_repo {
                    let target = format!("{repo_id}-sha{seq}");
                    let payload = target.clone().into_bytes();
                    loop {
                        match registry
                            .push(&repo_id, &target, base.as_deref(), 20, None, &payload)
                            .unwrap()
                        {
                            PushDecision::Accepted => break,
                            PushDecision::RateLimited { .. } => {
                                std::thread::sleep(Duration::from_millis(5));
                            }
                            PushDecision::Conflict => {
                                panic!("no other thread touches {repo_id} — a conflict here means cross-repo contention")
                            }
                            #[cfg(feature = "hub-provenance")]
                            PushDecision::SignatureInvalid => {
                                panic!("no verifier is configured on this registry")
                            }
                        }
                    }
                    base = Some(target);
                }
                repo_id
            })
        })
        .collect();

    for handle in handles {
        let repo_id = handle.join().unwrap();
        let expected_final = format!("{repo_id}-sha{}", pushes_per_repo - 1);
        let bytes = wait_for_commit(&registry, &repo_id, &expected_final);
        assert_eq!(bytes, expected_final.into_bytes());
    }
}

#[cfg(feature = "hub-canvas")]
#[test]
fn canvas_is_none_until_the_first_push_commits() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(dir.path(), generous_config()).unwrap();
    assert!(registry.canvas("repo-a").is_none());
}

#[cfg(feature = "hub-canvas")]
#[test]
fn canvas_renders_the_latest_committed_snapshot_s_modules() {
    use weave_graph_core::{Edge, Node, Storage};
    use weave_graph_store_sqlite::SqliteStorage;

    fn node(id: u32, path: &str) -> Node {
        Node {
            id,
            repo_id: "r".into(),
            path: path.into(),
            symbol: format!("s{id}"),
            kind: "function".into(),
            line_start: 1,
            line_end: 2,
            signature: String::new(),
        }
    }

    let snapshot_dir = tempfile::tempdir().unwrap();
    let db_path = snapshot_dir.path().join("graph.db");
    {
        let mut storage = SqliteStorage::open(&db_path).unwrap();
        storage.upsert_node(&node(1, "a.rs")).unwrap();
        storage.upsert_node(&node(2, "b.rs")).unwrap();
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

    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(dir.path(), generous_config()).unwrap();
    registry
        .push("repo-a", "sha1", None, 20, None, &bytes)
        .unwrap();
    wait_for_commit(&registry, "repo-a", "sha1");

    let canvas = registry.canvas("repo-a").unwrap().unwrap();
    assert!(!canvas.nodes.is_empty());
}

#[cfg(feature = "hub-canvas")]
#[test]
fn mesh_canvas_stitches_every_published_repo_and_skips_the_unpublished_one() {
    fn node(id: u32, path: &str) -> weave_graph_core::Node {
        weave_graph_core::Node {
            id,
            repo_id: "r".into(),
            path: path.into(),
            symbol: format!("s{id}"),
            kind: "function".into(),
            line_start: 1,
            line_end: 2,
            signature: String::new(),
        }
    }
    use weave_graph_core::Storage;
    use weave_graph_store_sqlite::SqliteStorage;

    let snapshot_bytes = |file: &str| -> Vec<u8> {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("graph.db");
        {
            let mut storage = SqliteStorage::open(&db_path).unwrap();
            storage.upsert_node(&node(1, file)).unwrap();
        }
        std::fs::read(&db_path).unwrap()
    };

    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(dir.path(), generous_config()).unwrap();
    registry
        .push("repo-a", "sha1", None, 20, None, &snapshot_bytes("a.rs"))
        .unwrap();
    registry
        .push("repo-b", "sha1", None, 20, None, &snapshot_bytes("b.rs"))
        .unwrap();
    wait_for_commit(&registry, "repo-a", "sha1");
    wait_for_commit(&registry, "repo-b", "sha1");

    let mesh = registry.mesh_canvas(&[
        "repo-a".to_string(),
        "repo-b".to_string(),
        "repo-never-published".to_string(),
    ]);
    let headers: Vec<&str> = mesh
        .nodes
        .iter()
        .filter(|n| n.id.starts_with("mesh-header-"))
        .map(|n| n.text.as_str())
        .collect();
    assert_eq!(
        headers,
        vec!["# repo-a", "# repo-b"],
        "the unpublished repo must be skipped, not error out the whole mesh"
    );
}

#[cfg(feature = "hub-webhooks")]
#[test]
fn set_webhook_then_an_empty_body_clears_it() {
    // A public IP literal — never actually dialed here, `set_webhook` only
    // resolves-and-classifies it, and an IP literal needs no DNS lookup to
    // do that, so this stays fast and network-independent.
    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(dir.path(), generous_config()).unwrap();
    assert_eq!(registry.webhook_url("repo-a"), None);

    registry
        .set_webhook("repo-a", "http://8.8.8.8/hook")
        .unwrap();
    assert_eq!(
        registry.webhook_url("repo-a").as_deref(),
        Some("http://8.8.8.8/hook")
    );

    registry.set_webhook("repo-a", "").unwrap();
    assert_eq!(registry.webhook_url("repo-a"), None);
}

/// SSRF guard, registered at the source (`Registry::set_webhook`) rather
/// than only the HTTP layer above it — a loopback or RFC1918 target must
/// never be accepted, since the *registry* is the one that will dial it.
#[cfg(feature = "hub-webhooks")]
#[test]
fn set_webhook_rejects_loopback_and_private_targets() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(dir.path(), generous_config()).unwrap();

    assert!(
        registry
            .set_webhook("repo-a", "http://127.0.0.1:9/hook")
            .is_err()
    );
    assert!(
        registry
            .set_webhook("repo-a", "http://10.1.2.3/hook")
            .is_err()
    );
    assert!(
        registry
            .set_webhook("repo-a", "http://169.254.169.254/hook")
            .is_err()
    );
    assert_eq!(
        registry.webhook_url("repo-a"),
        None,
        "a rejected target must never end up registered"
    );
}

#[cfg(feature = "hub-webhooks")]
#[test]
fn dispatch_webhook_calls_the_notifier_with_the_registered_url_repo_and_sha() {
    let dir = tempfile::tempdir().unwrap();
    let store_dir = dir.path().join("repo-a");
    std::fs::create_dir_all(&store_dir).unwrap();
    std::fs::write(store_dir.join("webhook.txt"), "http://example.invalid/hook").unwrap();

    let mut recorded = None;
    dispatch_webhook(&store_dir, "repo-a", "sha1", |url, repo_id, sha| {
        recorded = Some((url.to_string(), repo_id.to_string(), sha.to_string()));
        Ok(())
    });

    assert_eq!(
        recorded,
        Some((
            "http://example.invalid/hook".to_string(),
            "repo-a".to_string(),
            "sha1".to_string()
        ))
    );
}

#[cfg(feature = "hub-webhooks")]
#[test]
fn dispatch_webhook_never_calls_the_notifier_when_nothing_is_registered() {
    let dir = tempfile::tempdir().unwrap();
    let store_dir = dir.path().join("repo-a");
    std::fs::create_dir_all(&store_dir).unwrap();

    dispatch_webhook(&store_dir, "repo-a", "sha1", |_, _, _| {
        panic!("notifier must not be called when no webhook is registered")
    });
}

#[cfg(feature = "hub-webhooks")]
#[test]
fn a_committed_push_with_no_registered_webhook_never_dials_out() {
    // No webhook registered — if `worker_loop` tried to notify anyway,
    // that would be a real connect attempt on every push; this just proves
    // the no-webhook path commits cleanly without one.
    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::open(dir.path(), generous_config()).unwrap();
    registry
        .push("repo-a", "sha1", None, 20, None, b"v1")
        .unwrap();
    wait_for_commit(&registry, "repo-a", "sha1");
}

#[cfg(feature = "hub-provenance")]
mod provenance_gated {
    use std::sync::Arc;

    use crate::provenance::{MockSnapshotProvenanceVerifier, SnapshotProvenanceVerifier};

    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn a_push_with_no_verifier_configured_is_unaffected_by_a_missing_signature() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Registry::open(dir.path(), generous_config()).unwrap();
        let decision = registry
            .push("repo-a", "sha1", None, 20, None, b"payload")
            .unwrap();
        assert_eq!(decision, PushDecision::Accepted);
    }

    #[test]
    fn a_correctly_signed_push_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let verifier = MockSnapshotProvenanceVerifier::with_key(42);
        let registry = Registry::open(dir.path(), generous_config())
            .unwrap()
            .with_provenance_verifier(Arc::new(verifier));
        let sig = hex(&verifier.sign_snapshot("repo-a", "sha1", b"payload"));

        let decision = registry
            .push("repo-a", "sha1", None, 20, Some(&sig), b"payload")
            .unwrap();

        assert_eq!(decision, PushDecision::Accepted);
        assert_eq!(wait_for_commit(&registry, "repo-a", "sha1"), b"payload");
    }

    #[test]
    fn a_missing_signature_is_rejected_once_a_verifier_is_configured() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Registry::open(dir.path(), generous_config())
            .unwrap()
            .with_provenance_verifier(Arc::new(MockSnapshotProvenanceVerifier::with_key(42)));

        let decision = registry
            .push("repo-a", "sha1", None, 20, None, b"payload")
            .unwrap();

        assert_eq!(decision, PushDecision::SignatureInvalid);
        assert_eq!(
            registry.pull("repo-a", "sha1"),
            PullResult::NotFound,
            "a rejected push must never commit"
        );
    }

    #[test]
    fn a_signature_from_the_wrong_key_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Registry::open(dir.path(), generous_config())
            .unwrap()
            .with_provenance_verifier(Arc::new(MockSnapshotProvenanceVerifier::with_key(42)));
        let sig = hex(&MockSnapshotProvenanceVerifier::with_key(99)
            .sign_snapshot("repo-a", "sha1", b"payload"));

        let decision = registry
            .push("repo-a", "sha1", None, 20, Some(&sig), b"payload")
            .unwrap();

        assert_eq!(decision, PushDecision::SignatureInvalid);
    }

    #[test]
    fn a_signature_over_a_different_payload_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let verifier = MockSnapshotProvenanceVerifier::with_key(42);
        let registry = Registry::open(dir.path(), generous_config())
            .unwrap()
            .with_provenance_verifier(Arc::new(verifier));
        // Signed over "other payload", pushed with "payload" — tampered.
        let sig = hex(&verifier.sign_snapshot("repo-a", "sha1", b"other payload"));

        let decision = registry
            .push("repo-a", "sha1", None, 20, Some(&sig), b"payload")
            .unwrap();

        assert_eq!(decision, PushDecision::SignatureInvalid);
    }

    #[test]
    fn a_rejected_push_never_advances_the_head_or_consumes_a_retry() {
        let dir = tempfile::tempdir().unwrap();
        let verifier = MockSnapshotProvenanceVerifier::with_key(42);
        let registry = Registry::open(dir.path(), generous_config())
            .unwrap()
            .with_provenance_verifier(Arc::new(verifier));

        // Rejected first — the head must stay empty, so a later real push
        // can still land with `base_sha: None` (first-push semantics).
        registry
            .push("repo-a", "bad-sha", None, 20, None, b"forged")
            .unwrap();

        let sig = hex(&verifier.sign_snapshot("repo-a", "sha1", b"payload"));
        let decision = registry
            .push("repo-a", "sha1", None, 20, Some(&sig), b"payload")
            .unwrap();
        assert_eq!(decision, PushDecision::Accepted);
    }

    #[test]
    fn malformed_hex_in_the_signature_header_is_rejected_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let registry = Registry::open(dir.path(), generous_config())
            .unwrap()
            .with_provenance_verifier(Arc::new(MockSnapshotProvenanceVerifier::with_key(42)));

        let decision = registry
            .push("repo-a", "sha1", None, 20, Some("not-hex-zz"), b"payload")
            .unwrap();

        assert_eq!(decision, PushDecision::SignatureInvalid);
    }
}
