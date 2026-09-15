use std::fs;

use weave_graph_core::{NoteTier, Storage};

use super::*;

fn fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let weave_dir = dir.path().join(".weave");
    fs::create_dir_all(&weave_dir).unwrap();
    let active_db = weave_dir.join("graph.db");
    (dir, weave_dir, active_db)
}

fn discover(root: &Path, names: &[&str]) -> Vec<std::path::PathBuf> {
    names.iter().map(|n| root.join(n)).collect()
}

fn recall(root: &Path) -> Vec<weave_graph_core::Note> {
    let storage = SqliteStorage::open(&active_db(root)).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    storage.recall_notes(now).unwrap()
}

/// Reattachment regression test: a note survives its
/// symbol's purge-and-reinsert (moniker unchanged, node id changed) and
/// stays retrievable by recall.
#[test]
#[cfg(feature = "notes")]
fn note_survives_its_symbols_purge_and_reinsert() {
    let (dir, weave_dir, active_db) = fixture();
    let root = dir.path();

    fs::write(
        root.join("a.rs"),
        "fn keepme() {}\nfn shifty() { keepme(); }\n",
    )
    .unwrap();
    let files = discover(root, &["a.rs"]);
    crate::index::full_reindex(root, &weave_dir, &active_db, &files).unwrap();

    let storage = SqliteStorage::open(&active_db).unwrap();
    let _shifty = resolve_symbol(&storage, "shifty").unwrap();
    // Crystallized: must survive the reindex to be reattachable at all.
    cmd_note_pin(root, "shifty", "why shifty exists", true, "note").unwrap();

    // Shift the symbol down: purge-and-reinsert gives it a new node id.
    fs::write(
        root.join("a.rs"),
        "fn keepme() {}\n\n// line added above\nfn shifty() { keepme(); }\n",
    )
    .unwrap();
    let changed = vec!["a.rs".to_string()];
    crate::index::incremental_reindex(root, &weave_dir, &active_db, &files, &changed).unwrap();

    let storage = SqliteStorage::open(&active_db).unwrap();
    let new_id = resolve_symbol(&storage, "shifty").unwrap().id;
    let recalled = recall(root);
    assert_eq!(recalled.len(), 1);
    assert_eq!(
        recalled[0].target_node_id,
        Some(new_id),
        "reattached to the NEW id"
    );
    assert!(
        !recalled[0].stale,
        "content untouched — only line position moved"
    );
}

/// A genuinely deleted symbol (moniker gone) orphans its note: reported
/// by recall, never silently dropped, never misattached to another node.
#[test]
#[cfg(feature = "notes")]
fn note_on_a_deleted_symbol_is_reported_orphaned() {
    let (dir, weave_dir, active_db) = fixture();
    let root = dir.path();

    fs::write(root.join("a.rs"), "fn doomed() {}\nfn survivor() {}\n").unwrap();
    let files = discover(root, &["a.rs"]);
    crate::index::full_reindex(root, &weave_dir, &active_db, &files).unwrap();

    cmd_note_pin(root, "doomed", "rationale", true, "note").unwrap();

    // Delete the symbol entirely.
    fs::write(root.join("a.rs"), "fn survivor() {}\n").unwrap();
    let changed = vec!["a.rs".to_string()];
    crate::index::incremental_reindex(root, &weave_dir, &active_db, &files, &changed).unwrap();

    let recalled = recall(root);
    assert_eq!(
        recalled.len(),
        1,
        "orphaned notes are reported, never dropped"
    );
    assert_eq!(
        recalled[0].target_node_id, None,
        "orphaned, never misattached"
    );
    assert_eq!(recalled[0].moniker, "a.rs#doomed");
}

#[test]
#[cfg(feature = "notes")]
fn ephemeral_notes_vanish_from_recall_after_their_ttl() {
    let (dir, _weave_dir, active_db) = fixture();
    let root = dir.path();
    // Pinned directly with a TTL already elapsed: recall filters it at
    // read time with no process running in between.
    // Scoped: `weave index` runs as a separate process in real life — the
    // pin connection must be closed before the rebuild swap replaces the
    // file, or its stale WAL sidecar resurrects old pages.
    {
        let storage = SqliteStorage::open(&active_db).unwrap();
        let note = weave_graph_core::Note {
            id: 0,
            target_node_id: None,
            moniker: "a.rs#gone".into(),
            kind: "note".into(),
            tier: NoteTier::Ephemeral,
            author: "agent".into(),
            content: "session scratchpad".into(),
            content_hash: None,
            stale: false,
            expires_at: Some(1), // long expired
            created_at: 0,
        };
        storage.pin_note(&note).unwrap();
    }
    assert_eq!(recall(root), vec![]);

    // The next index opportunistically deletes the expired row.
    fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
    let files = discover(root, &["a.rs"]);
    let weave_dir = root.join(".weave");
    crate::index::full_reindex(root, &weave_dir, &active_db, &files).unwrap();
    let storage = SqliteStorage::open(&active_db).unwrap();
    assert_eq!(
        storage.all_notes().unwrap().len(),
        0,
        "opportunistic delete during index"
    );
}
/// Crystallized note flagged `stale` after an equal-line-range content
/// rewrite; untouched content not flagged.
#[test]
#[cfg(feature = "notes")]
fn equal_line_range_rewrite_flags_staleness_and_untouched_content_does_not() {
    let (dir, weave_dir, active_db) = fixture();
    let root = dir.path();

    // Two symbols, equal line spans.
    fs::write(root.join("a.rs"), "fn alpha() {}\nfn beta() {}\n").unwrap();
    let files = discover(root, &["a.rs"]);
    crate::index::full_reindex(root, &weave_dir, &active_db, &files).unwrap();
    cmd_note_pin(root, "alpha", "alpha rationale", true, "note").unwrap();
    cmd_note_pin(root, "beta", "beta rationale", true, "note").unwrap();

    // Rewrite one symbol in place — identical line count, different content.
    fs::write(
        root.join("a.rs"),
        "fn alpha() { rewritten(); }\nfn beta() {}\n",
    )
    .unwrap();
    let changed = vec!["a.rs".to_string()];
    crate::index::incremental_reindex(root, &weave_dir, &active_db, &files, &changed).unwrap();

    let storage = SqliteStorage::open(&active_db).unwrap();
    let alpha = storage
        .all_notes()
        .unwrap()
        .into_iter()
        .find(|n| n.moniker == "a.rs#alpha")
        .unwrap();
    assert!(
        alpha.stale,
        "equal-line-range rewrite must be flagged stale"
    );

    let beta = storage
        .all_notes()
        .unwrap()
        .into_iter()
        .find(|n| n.moniker == "a.rs#beta")
        .unwrap();
    assert!(!beta.stale, "untouched content must not be flagged");
}

#[test]
#[cfg(feature = "notes")]
fn pinning_an_unknown_symbol_is_a_clear_error() {
    let (dir, weave_dir, active_db) = fixture();
    let root = dir.path();
    fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
    let files = discover(root, &["a.rs"]);
    crate::index::full_reindex(root, &weave_dir, &active_db, &files).unwrap();

    assert!(cmd_note_pin(root, "nope", "text", false, "note").is_err());
}
