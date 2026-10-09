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

fn extractor_version_path(weave_dir: &Path) -> PathBuf {
    weave_dir.join("extractor_version")
}

/// `None` covers both "never indexed" and "indexed by a build predating this
/// file" — both must be treated as a version mismatch by the caller, not as
/// a match, so a pre-existing index gets one safe full reindex rather than
/// silently trusting data an unversioned build produced.
pub(crate) fn read_extractor_version(weave_dir: &Path) -> Option<u32> {
    fs::read_to_string(extractor_version_path(weave_dir))
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

pub(crate) fn write_extractor_version(weave_dir: &Path, version: u32) -> std::io::Result<()> {
    fs::write(extractor_version_path(weave_dir), version.to_string())
}

/// Wipes every cached commit snapshot (not `last_indexed_sha`/
/// `extractor_version` themselves) — every snapshot was built by whatever
/// extractor was running when it was saved, with no per-snapshot version
/// tag, so after an `EXTRACTOR_VERSION` mismatch they're all suspect at
/// once. A no-op if the cache directory doesn't exist yet.
pub(crate) fn clear_snapshot_cache(weave_dir: &Path) -> std::io::Result<()> {
    match fs::remove_dir_all(weave_dir.join("cache")) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
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
