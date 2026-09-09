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
