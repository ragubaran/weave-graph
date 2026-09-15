use std::fs;
use std::path::{Path, PathBuf};

fn snapshot_path(weave_dir: &Path, sha: &str) -> PathBuf {
    weave_dir.join("cache").join(format!("{sha}.idx"))
}

fn last_indexed_sha_path(weave_dir: &Path) -> PathBuf {
    weave_dir.join("last_indexed_sha")
}

pub(crate) fn read_last_indexed_sha(weave_dir: &Path) -> Option<String> {
    fs::read_to_string(last_indexed_sha_path(weave_dir))
        .ok()
        .map(|s| s.trim().to_string())
}

pub(crate) fn write_last_indexed_sha(weave_dir: &Path, sha: &str) -> std::io::Result<()> {
    fs::write(last_indexed_sha_path(weave_dir), sha)
}

/// Restores `active_db` from the cached snapshot for `sha`, through the same
/// build-into-`.rebuild`-then-atomically-rename path as a full rebuild (Core
/// Invariant 2) — a cache hit must never leave `active_db` half-written
/// either. Returns `false` (no-op) if no snapshot exists for `sha`.
pub(crate) fn restore_snapshot(
    weave_dir: &Path,
    active_db: &Path,
    sha: &str,
) -> std::io::Result<bool> {
    let snapshot = snapshot_path(weave_dir, sha);
    if !snapshot.exists() {
        return Ok(false);
    }
    let rebuild_db = weave_dir.join("graph.db.rebuild");
    fs::copy(&snapshot, &rebuild_db)?;
    fs::rename(&rebuild_db, active_db)?;
    Ok(true)
}

/// Saves the just-built `active_db` as the commit-hash snapshot for `sha`, so a
/// later `weave index` back on this exact commit can restore instead of
/// reparsing (fast branch switching). Callers only invoke this on a clean
/// working tree — a dirty tree's on-disk content doesn't match `sha`, so
/// caching it under `sha` would be wrong.
pub(crate) fn save_snapshot(weave_dir: &Path, active_db: &Path, sha: &str) -> std::io::Result<()> {
    let cache_dir = weave_dir.join("cache");
    fs::create_dir_all(&cache_dir)?;
    fs::copy(active_db, snapshot_path(weave_dir, sha))?;
    Ok(())
}

#[cfg(test)]
mod tests;
