use std::fs;

use super::*;

#[test]
fn last_indexed_sha_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    assert!(read_last_indexed_sha(dir.path()).is_none());

    write_last_indexed_sha(dir.path(), "abc123").unwrap();
    assert_eq!(read_last_indexed_sha(dir.path()).as_deref(), Some("abc123"));
}

#[test]
fn extractor_version_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    assert!(read_extractor_version(dir.path()).is_none());

    write_extractor_version(dir.path(), 1).unwrap();
    assert_eq!(read_extractor_version(dir.path()), Some(1));
}

#[test]
fn extractor_version_mismatch_is_not_a_silent_match() {
    let dir = tempfile::tempdir().unwrap();
    write_extractor_version(dir.path(), 1).unwrap();
    assert_ne!(read_extractor_version(dir.path()), Some(2));
}

#[test]
fn clear_snapshot_cache_is_a_no_op_when_nothing_was_ever_cached() {
    let dir = tempfile::tempdir().unwrap();
    clear_snapshot_cache(dir.path()).unwrap();
}

#[test]
fn clear_snapshot_cache_removes_every_cached_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let active_db = dir.path().join("graph.db");
    fs::write(&active_db, b"content").unwrap();
    save_snapshot(dir.path(), &active_db, "sha1").unwrap();
    save_snapshot(dir.path(), &active_db, "sha2").unwrap();

    clear_snapshot_cache(dir.path()).unwrap();

    assert!(!restore_snapshot(dir.path(), &active_db, "sha1").unwrap());
    assert!(!restore_snapshot(dir.path(), &active_db, "sha2").unwrap());
}

#[test]
fn restore_snapshot_returns_false_when_no_cache_exists() {
    let dir = tempfile::tempdir().unwrap();
    let active_db = dir.path().join("graph.db");
    assert!(!restore_snapshot(dir.path(), &active_db, "deadbeef").unwrap());
    assert!(!active_db.exists());
}

#[test]
fn save_then_restore_snapshot_round_trips_the_database_content() {
    let dir = tempfile::tempdir().unwrap();
    let active_db = dir.path().join("graph.db");
    fs::write(&active_db, b"fake db content at commit abc").unwrap();

    save_snapshot(dir.path(), &active_db, "abc").unwrap();
    fs::write(&active_db, b"different content, e.g. after a branch switch").unwrap();

    let restored = restore_snapshot(dir.path(), &active_db, "abc").unwrap();
    assert!(restored);
    assert_eq!(
        fs::read(&active_db).unwrap(),
        b"fake db content at commit abc"
    );
}

#[test]
fn restore_never_leaves_a_leftover_rebuild_file() {
    let dir = tempfile::tempdir().unwrap();
    let active_db = dir.path().join("graph.db");
    fs::write(&active_db, b"content").unwrap();
    save_snapshot(dir.path(), &active_db, "sha1").unwrap();

    restore_snapshot(dir.path(), &active_db, "sha1").unwrap();
    assert!(!dir.path().join("graph.db.rebuild").exists());
}
