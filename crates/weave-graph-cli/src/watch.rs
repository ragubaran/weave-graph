//! `--features watch`: file-watcher auto-sync with
//! blast-radius gating. `notify` reports raw filesystem events; this module
//! debounces bursts of them into one batch per quiet period, gates each
//! batch's blast radius (union of `reachable_within` over the changed
//! files' already-indexed symbols) against `[watch] blast_radius_ceiling`,
//! and defers anything larger behind a visible `.weave/pending-manual-reindex`
//! marker rather than silently skipping or auto-reindexing regardless.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use weave_graph_core::{CsrGraph, NodeId, Storage, StorageError};

use crate::config;

pub(crate) struct WatchConfig {
    pub(crate) debounce_ms: u64,
    pub(crate) blast_radius_ceiling: usize,
}

impl WatchConfig {
    /// `debounce_ms` defaults to 2000, clamped to `[100, 60_000]` to match
    /// `ReindexConfig`'s own existing clamping discipline.
    /// `blast_radius_ceiling` defaults to 200, matching the report canvas's
    /// 200-node budget — a stated guess, not a measured number.
    pub(crate) fn load(root: &Path) -> Self {
        let path = config_path(root);
        let debounce_ms = config::get_key(&path, "watch.debounce_ms")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(2000)
            .clamp(100, 60_000);
        let blast_radius_ceiling = config::get_key(&path, "watch.blast_radius_ceiling")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(200);
        Self {
            debounce_ms,
            blast_radius_ceiling,
        }
    }
}

fn config_path(root: &Path) -> PathBuf {
    root.join(".weave").join("config.toml")
}

/// `[watch] enabled = true` opts a repo into the background watcher inside
/// `weave serve --mcp`. Absent or any other value means off — enabling the
/// `watch` Cargo feature must never turn this on by itself.
pub(crate) fn enabled(root: &Path) -> bool {
    config::get_key(&config_path(root), "watch.enabled").as_deref() == Some("true")
}

/// Names the changed files and the computed blast-radius size a deferred
/// batch was too large to auto-reindex — surfaced through `weave status`
/// and MCP tool responses, re-evaluated on every subsequent tick, and
/// cleared the moment a `weave index` run succeeds.
#[derive(Serialize, Deserialize)]
pub(crate) struct PendingMarker {
    pub(crate) files: Vec<String>,
    pub(crate) blast_radius: usize,
}

fn marker_path(weave_dir: &Path) -> PathBuf {
    weave_dir.join("pending-manual-reindex")
}

pub(crate) fn read_pending_marker(weave_dir: &Path) -> Option<PendingMarker> {
    let content = std::fs::read_to_string(marker_path(weave_dir)).ok()?;
    serde_json::from_str(&content).ok()
}

pub(crate) fn write_pending_marker(
    weave_dir: &Path,
    marker: &PendingMarker,
) -> std::io::Result<()> {
    let content = serde_json::to_string_pretty(marker).map_err(std::io::Error::other)?;
    std::fs::write(marker_path(weave_dir), content)
}

/// Never errors on a missing marker — clearing an already-clear marker
/// (the common case: most reindexes never deferred) is not a failure.
pub(crate) fn clear_pending_marker(weave_dir: &Path) {
    let _ = std::fs::remove_file(marker_path(weave_dir));
}

/// Names files seen changed but still inside the current debounce window —
/// distinct from [`PendingMarker`]: this is the transient "not reindexed
/// *yet*" case, not the "too large to auto-reindex" case. MCP tool
/// responses surface both, worded differently.
fn in_flight_path(weave_dir: &Path) -> PathBuf {
    weave_dir.join("watch-in-flight")
}

pub(crate) fn write_in_flight(weave_dir: &Path, files: &std::collections::BTreeSet<String>) {
    let content = serde_json::to_string(&files.iter().collect::<Vec<_>>()).unwrap_or_default();
    let _ = std::fs::write(in_flight_path(weave_dir), content);
}

pub(crate) fn clear_in_flight(weave_dir: &Path) {
    let _ = std::fs::remove_file(in_flight_path(weave_dir));
}

pub(crate) fn read_in_flight(weave_dir: &Path) -> Vec<String> {
    std::fs::read_to_string(in_flight_path(weave_dir))
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

/// Union of `reachable_within` (unbounded hop count, matching
/// `weave_impact_radius`'s own "full BFS, no depth cap" convention) over
/// every already-indexed symbol in `changed_files` — the "how much would
/// auto-reindexing touch" number `[watch] blast_radius_ceiling` gates on.
pub(crate) fn blast_radius(
    storage: &dyn Storage,
    csr: &CsrGraph,
    changed_files: &[String],
) -> Result<usize, StorageError> {
    let changed: HashSet<&str> = changed_files.iter().map(String::as_str).collect();
    let mut seeds: Vec<NodeId> = Vec::new();
    storage.for_each_node(&mut |node| {
        if changed.contains(node.path.as_str()) {
            seeds.push(node.id);
        }
    })?;
    let mut reached = roaring::RoaringBitmap::new();
    for id in seeds {
        reached |= csr.reachable_within(id, u32::MAX);
    }
    Ok(reached.len() as usize)
}

/// Blocks on `events`, debouncing bursts into one `on_batch` call per quiet
/// period of `debounce_ms` — this is what turns a thousand-event storm into
/// exactly one reindex attempt. "Coalesce while busy" falls out for free:
/// events that arrive while `on_batch` is still running accumulate on the
/// (unbounded) channel, and the very next loop iteration picks them up and
/// debounces them again rather than firing a second overlapping attempt.
/// Each batch carries the real paths `notify` reported for that debounce
/// window — deliberately not a git diff: that would silently never fire in
/// a repo with no commit to diff against, where a raw filesystem event is
/// still real, actionable information. `on_event` fires once per accumulated
/// path *before* the batch is complete — the only way to surface "still
/// inside the debounce window" staleness, since that state doesn't exist
/// once `on_batch` runs. Returns once `events` disconnects.
pub(crate) fn run(
    events: &Receiver<PathBuf>,
    debounce_ms: u64,
    mut on_event: impl FnMut(&PathBuf),
    mut on_batch: impl FnMut(&std::collections::BTreeSet<PathBuf>),
) {
    let debounce = Duration::from_millis(debounce_ms);
    loop {
        let mut batch = std::collections::BTreeSet::new();
        match events.recv() {
            Ok(p) => {
                on_event(&p);
                batch.insert(p);
            }
            Err(_) => return,
        }
        loop {
            match events.recv_timeout(debounce) {
                Ok(p) => {
                    on_event(&p);
                    batch.insert(p);
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    on_batch(&batch);
                    return;
                }
            }
        }
        on_batch(&batch);
    }
}

#[cfg(test)]
mod tests;
