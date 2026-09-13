//! `weave sync pull|push` behind the `hub` Cargo feature. Pull hydrates
//! a hub snapshot into `graph.db` through the
//! same `.rebuild`-then-atomic-rename path as every other write (Core
//! Invariant 2); push publishes merge-only — never from a feature branch —
//! and treats `409` as "republish the full snapshot", since graphs are
//! derived data and recompute-and-overwrite is always correct.
//!
//! **v1 scope, stated**: the payload is the full `graph.db` (uncompressed —
//! zstd is follow-on scope), sent with the delta envelope's `base_commit_sha`
//! as a header. The hub fast-forwards on match, `409`s on mismatch; the
//! client republishes once and never rebases server-side.

use std::fs;
use std::path::{Path, PathBuf};

use weave_graph_hub::{HubClient, PullOutcome, PushOutcome};
use weave_graph_store_sqlite::SqliteStorage;

use crate::cache;
use crate::config;

fn hub_url(root: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let url = config::get_key(&root.join(".weave").join("config.toml"), "hub.url")
        .filter(|u| !u.is_empty());
    url.ok_or_else(|| {
        "No hub configured: set [hub] url in .weave/config.toml. Leaving it unset is a \
         fully supported permanent state — sync is opt-in."
            .into()
    })
}

/// `[hub] token` (HUB-02) — omitted entirely for the still-fully-supported
/// unauthenticated deployment; only attached when a deployment actually
/// runs `weave-registry --auth-token`.
fn hub_token(root: &Path) -> Option<String> {
    config::get_key(&root.join(".weave").join("config.toml"), "hub.token").filter(|t| !t.is_empty())
}

fn hub_client(root: &Path) -> Result<HubClient, Box<dyn std::error::Error>> {
    let client = HubClient::new(&hub_url(root)?, &repo_label(root))?;
    Ok(match hub_token(root) {
        Some(token) => client.with_token(token),
        None => client,
    })
}

fn repo_label(root: &Path) -> String {
    root.canonicalize()
        .unwrap_or_else(|_| root.to_path_buf())
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| root.to_string_lossy().to_string())
}

fn retention(root: &Path) -> usize {
    config::get_key(
        &root.join(".weave").join("config.toml"),
        "hub.snapshot_retention",
    )
    .and_then(|v| v.parse::<usize>().ok())
    .unwrap_or(20)
}

fn data_db_path(root: &Path) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let weave_home_env = std::env::var("WEAVE_HOME").ok();
    let data_dir = crate::storage_location::resolve_data_dir(root, weave_home_env.as_deref());
    Ok(data_dir.path.join("graph.db"))
}

struct PushSnapshot {
    path: PathBuf,
}

impl Drop for PushSnapshot {
    fn drop(&mut self) {
        remove_snapshot(&self.path);
    }
}

fn remove_snapshot(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(format!("{}-wal", path.display()));
    let _ = fs::remove_file(format!("{}-shm", path.display()));
}

fn snapshot_for_push(
    root: &Path,
    db_path: &Path,
) -> Result<PushSnapshot, Box<dyn std::error::Error>> {
    let snapshot_path = db_path.with_extension("db.push-tmp");
    remove_snapshot(&snapshot_path);
    let source = SqliteStorage::open(db_path)?;
    source.export_read_only_snapshot(&snapshot_path)?;
    drop(source);

    #[cfg(not(feature = "vector"))]
    let _ = root;

    #[cfg(feature = "vector")]
    {
        let excluded_paths =
            crate::config::read_vector_exclude(&root.join(".weave").join("config.toml"));
        if !excluded_paths.is_empty() {
            let sanitized_path = db_path.with_extension("db.push-sanitized");
            remove_snapshot(&sanitized_path);
            let snapshot = SqliteStorage::open(&snapshot_path)?;
            snapshot.purge_vector_paths(&excluded_paths)?;
            snapshot.export_read_only_snapshot(&sanitized_path)?;
            drop(snapshot);
            remove_snapshot(&snapshot_path);
            return Ok(PushSnapshot {
                path: sanitized_path,
            });
        }
    }

    Ok(PushSnapshot {
        path: snapshot_path,
    })
}

/// `weave sync pull [--commit <sha>] [--fallback-latest]`: hydrate the hub's
/// snapshot for the merge-base commit (or an explicit one) into `graph.db`.
/// Merge-base anchoring means a PR runner hydrates the graph as of the
/// branch point, then `weave index --incremental` pays only the delta.
pub(crate) fn cmd_sync_pull(
    root: &Path,
    commit: Option<&str>,
    fallback_latest: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = hub_client(root)?;
    let sha = match commit {
        Some(explicit) => explicit.to_string(),
        None => git_merge_base(root)?,
    };

    // `weave` ships no `SnapshotProvenanceVerifier` of its own — verifying
    // a signature is a deployment concern (`weave_graph_hub`'s trait is
    // public under `hub-provenance` for exactly that integration), the
    // same boundary this crate's doc provenance already draws elsewhere.
    // What this crate *can* do honestly is stop discarding the signature
    // the registry already returns — surfaced below instead of bound to `_`.
    let bytes = match client.pull(&sha)? {
        PullOutcome::Found(bytes, signature) => {
            report_signature(&signature);
            bytes
        }
        PullOutcome::NotFound if fallback_latest => match client.pull("latest")? {
            PullOutcome::Found(bytes, signature) => {
                println!("No snapshot for {sha}; hydrated latest instead.");
                report_signature(&signature);
                bytes
            }
            PullOutcome::NotFound => {
                println!("No snapshot for {sha} and no latest snapshot on the hub.");
                return Ok(());
            }
        },
        PullOutcome::NotFound => {
            println!("No snapshot for {sha} on the hub (use --fallback-latest to hydrate latest).");
            return Ok(());
        }
    };

    // Core Invariant 2: never write the active db in place — stage into
    // `.rebuild`, then atomically rename over it.
    let db_path = data_db_path(root)?;
    let rebuild = db_path.with_extension("db.rebuild");
    fs::write(&rebuild, &bytes)?;

    // Enforce filesystem-specific pragmas (WAL vs DELETE) based on the mount
    // before the active database sees this file (Gap 5).
    if let Ok(storage) = weave_graph_store_sqlite::SqliteStorage::open(&rebuild) {
        let _ = storage.checkpoint_wal();
    }

    fs::rename(&rebuild, &db_path)?;
    println!(
        "Hydrated snapshot for {sha} into {} ({} bytes). Run `weave index --incremental` \
         to fast-forward to HEAD.",
        db_path.display(),
        bytes.len()
    );
    Ok(())
}

/// Surfaces a pulled snapshot's recorded signature rather than discarding
/// it — this crate verifies nothing (no `SnapshotProvenanceVerifier` is
/// wired in), so "present" is reported as a fact, never as "verified".
fn report_signature(signature: &Option<String>) {
    match signature {
        Some(sig) => println!(
            "Snapshot signature on record: {sig} (not verified — `weave` ships no \
             provenance verifier; wire one via `weave_graph_hub::SnapshotProvenanceVerifier` \
             if your deployment needs to check it)."
        ),
        None => println!("Snapshot has no recorded signature."),
    }
}

/// `weave sync push`: publish the current graph snapshot. Merge-only by
/// construction — a feature branch is refused before any network call. The
/// v1 payload is always the full snapshot (graphs are derived data;
/// recompute-and-overwrite is the correct resolution on `409`), with the
/// delta envelope's `base_commit_sha` carried as a header for the hub's
/// fast-forward decision.
///
/// `signature` comes from `weave sync push --signature <sig>` (or `None`
/// if the flag is omitted, the default) — this crate ships no
/// `SnapshotProvenanceVerifier` of its own (signing is a deployment
/// concern, the same boundary this crate's doc provenance draws
/// elsewhere), so there is nothing built in to sign with. The flag is the
/// seam: an operator with a real signature (computed via
/// `weave_graph_hub`'s public, `hub-provenance`-gated trait, or any other
/// external signer) hands it in here rather than `weave` ever computing
/// one itself.
pub(crate) fn cmd_sync_push(
    root: &Path,
    signature: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let branch = crate::git::current_branch(root);
    match branch.as_deref() {
        Some("main") | Some("master") => {}
        Some(other) => {
            return Err(format!(
                "Refusing to publish from branch '{other}' — the hub accepts publishes \
                 only on the default branch (merge-only publish)."
            )
            .into());
        }
        None => {
            return Err(
                "Not on a git branch (detached HEAD or not a repo) — refusing to publish.".into(),
            );
        }
    }

    let client = hub_client(root)?;
    let target = crate::git::current_sha(root)
        .ok_or("Cannot determine HEAD commit — refusing to publish without a commit sha")?;
    let base = cache::read_last_indexed_sha(&root.join(".weave"));
    let db_path = data_db_path(root)?;

    let snapshot = snapshot_for_push(root, &db_path)?;

    let outcome = push_with_backoff(
        &client,
        &target,
        base.as_deref(),
        retention(root),
        signature,
        &snapshot.path,
    )?;
    match outcome {
        PushOutcome::Published | PushOutcome::Accepted => {
            println!("Published snapshot for {target}.")
        }
        PushOutcome::Conflict => {
            // v1 always sends the full snapshot, so a conflict is resolved by
            // republishing it — the hub's head simply moved. Bounded retry
            // loop with the same backoff+jitter as the rate-limit path
            // (not just a single unconditional retry) since a busy hub can
            // race a second concurrent publish into the same window.
            let mut outcome = PushOutcome::Conflict;
            for attempt in 0..MAX_CONFLICT_RETRIES {
                std::thread::sleep(std::time::Duration::from_millis(jitter_ms()));
                outcome = push_with_backoff(
                    &client,
                    &target,
                    None,
                    retention(root),
                    signature,
                    &snapshot.path,
                )?;
                if !matches!(outcome, PushOutcome::Conflict) {
                    break;
                }
                eprintln!(
                    "Hub head still moving (attempt {}/{MAX_CONFLICT_RETRIES}); retrying.",
                    attempt + 1
                );
            }
            match outcome {
                PushOutcome::Published | PushOutcome::Accepted => {
                    println!("Hub head had moved; republished full snapshot for {target}.")
                }
                PushOutcome::Conflict => {
                    return Err(format!(
                        "Hub still refuses the snapshot after {MAX_CONFLICT_RETRIES} republish attempts"
                    )
                    .into());
                }
                PushOutcome::RateLimited { .. } => {
                    return Err("Hub rate-limited the publish during conflict republish".into());
                }
            }
        }
        PushOutcome::RateLimited { retry_after_secs } => {
            let wait = retry_after_secs
                .map(|s| format!("{s}s"))
                .unwrap_or_else(|| "an unspecified interval".to_string());
            return Err(format!(
                "Hub rate-limited the publish; still limited after {MAX_PUSH_ATTEMPTS} \
                 attempts with exponential backoff (last wait: {wait})"
            )
            .into());
        }
    }
    Ok(())
}

/// Runners back off exponentially with jitter on `429` instead of failing
/// on the first rate-limit response — the registry's watermark is
/// expected to clear within a few seconds
/// under ordinary load. Jitter avoids a thundering herd of CI runners all
/// retrying at the exact same instant; it's derived from wall-clock
/// nanoseconds rather than a `rand` dependency this crate doesn't need
/// elsewhere.
const MAX_PUSH_ATTEMPTS: u32 = 5;

/// Bounded retries for a `409 Conflict` republish (retry-with-backoff,
/// not a single unconditional retry).
const MAX_CONFLICT_RETRIES: u32 = 3;

/// Sub-second jitter derived from wall-clock nanoseconds, so concurrent
/// runners retrying at the same instant don't stay lockstepped — no `rand`
/// dependency needed for this.
fn jitter_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::from(d.subsec_nanos() % 1000))
        .unwrap_or(0)
}

fn push_with_backoff(
    client: &weave_graph_hub::HubClient,
    target_sha: &str,
    base_sha: Option<&str>,
    retention: usize,
    signature: Option<&str>,
    snapshot_path: &Path,
) -> Result<PushOutcome, Box<dyn std::error::Error>> {
    for attempt in 0..MAX_PUSH_ATTEMPTS {
        let outcome =
            client.push_file(target_sha, base_sha, retention, signature, snapshot_path)?;
        let PushOutcome::RateLimited { retry_after_secs } = outcome else {
            return Ok(outcome);
        };
        if attempt + 1 == MAX_PUSH_ATTEMPTS {
            return Ok(outcome);
        }
        let base_wait = retry_after_secs.unwrap_or(1);
        let backoff = base_wait.saturating_mul(1u64 << attempt);
        let jitter_ms = jitter_ms();
        eprintln!(
            "Hub rate-limited (attempt {}/{MAX_PUSH_ATTEMPTS}); backing off {backoff}s + {jitter_ms}ms jitter.",
            attempt + 1
        );
        std::thread::sleep(
            std::time::Duration::from_secs(backoff) + std::time::Duration::from_millis(jitter_ms),
        );
    }
    unreachable!("loop always returns by the last attempt")
}

/// `git merge-base <ref> HEAD` — the merge-base anchor `weave sync pull`
/// needs. A shallow checkout (`fetch-depth: 1`) has no common ancestor, so
/// the error names the fix instead of surfacing a raw git failure.
fn git_merge_base(root: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["merge-base", "origin/main", "HEAD"])
        .output()?;
    if !output.status.success() {
        return Err(
            "git merge-base origin/main HEAD failed — a shallow checkout (e.g. \
             actions/checkout's default fetch-depth: 1) has no common ancestor. \
             Set fetch-depth: 0 (or deep enough to reach the merge-base) in CI."
                .into(),
        );
    }
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if sha.is_empty() {
        return Err("git merge-base produced no output".into());
    }
    Ok(sha)
}

#[cfg(test)]
mod tests;
