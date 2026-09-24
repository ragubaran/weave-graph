use super::*;
use weave_graph_core::schema::LATEST_SCHEMA_VERSION;

fn node(repo: &str, path: &str, symbol: &str, line_start: u32) -> Node {
    Node {
        id: 0,
        repo_id: repo.into(),
        path: path.into(),
        symbol: symbol.into(),
        kind: "function".into(),
        line_start,
        line_end: line_start + 2,
        signature: format!("fn {symbol}()"),
    }
}

fn edge(source_id: NodeId, target_id: NodeId, kind: &str) -> Edge {
    Edge {
        id: 0,
        source_id,
        target_id,
        kind: kind.into(),
        weight: 1.0,
        extractor: None,
        resolution_kind: None,
    }
}

#[test]
fn upsert_and_get_node_round_trips() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let id = storage
        .upsert_node(&node("r", "src/lib.rs", "foo", 1))
        .unwrap();

    let fetched = storage.get_node(id).unwrap().unwrap();
    assert_eq!(fetched.symbol, "foo");
    assert_eq!(fetched.path, "src/lib.rs");
}

#[test]
fn get_node_returns_none_for_unknown_id() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    assert_eq!(storage.get_node(12345).unwrap(), None);
}

#[test]
fn upsert_node_on_same_natural_key_updates_in_place() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let first = node("r", "src/lib.rs", "foo", 1);
    let id1 = storage.upsert_node(&first).unwrap();

    let mut changed = first.clone();
    changed.signature = "fn foo() -> i32".into();
    let id2 = storage.upsert_node(&changed).unwrap();

    assert_eq!(id1, id2, "same natural key must not create a second row");
    assert_eq!(
        storage.get_node(id1).unwrap().unwrap().signature,
        "fn foo() -> i32"
    );
}

#[test]
fn fresh_batch_insert_reuses_the_natural_key_without_semantic_payload() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let first = node("r", "src/lib.rs", "foo", 1);
    let mut changed = first.clone();
    changed.signature = "fn foo() -> i32".into();

    let first_id = storage.insert_fresh_nodes(&[first]).unwrap()[0];
    let changed_id = storage.insert_fresh_nodes(&[changed]).unwrap()[0];
    assert_eq!(first_id, changed_id);
    assert_eq!(
        storage.get_node(first_id).unwrap().unwrap().signature,
        "fn foo() -> i32"
    );
    let duplicate: Option<String> = storage
        .conn
        .query_row(
            "SELECT semantic_key FROM nodes WHERE id = ?1",
            [first_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(duplicate, None);
}

#[test]
fn upsert_node_preserves_identity_when_only_its_span_moves() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let first = node("r", "src/lib.rs", "foo", 10);
    let id = storage.upsert_node(&first).unwrap();

    let mut shifted = first;
    shifted.line_start = 50;
    shifted.line_end = 52;
    assert_eq!(storage.upsert_node(&shifted).unwrap(), id);
    assert_eq!(storage.get_node(id).unwrap().unwrap().line_start, 50);
}

/// Indexed identity columns preserve IDs without duplicating their text.
#[test]
fn semantic_key_preserves_identity_across_span_moves_and_resolves_overloads() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let first = node("r", "a.rs", "foo", 1);
    let id = storage.upsert_node(&first).unwrap();
    let duplicate: Option<String> = storage
        .conn
        .query_row(
            "SELECT semantic_key FROM nodes WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(duplicate, None);

    // Line span moves, signature unchanged: same semantic identity.
    let mut shifted = first.clone();
    shifted.line_start = 50;
    shifted.line_end = 52;
    assert_eq!(storage.upsert_node(&shifted).unwrap(), id);
    assert_eq!(storage.get_node(id).unwrap().unwrap().signature, "fn foo()");

    // Two same-name overloads are distinct identities.
    let mut overload = first;
    overload.line_start = 10;
    overload.signature = "fn foo(&str)".into();
    let overload_id = storage.upsert_node(&overload).unwrap();
    assert_ne!(id, overload_id);
    assert_eq!(
        storage.get_node(overload_id).unwrap().unwrap().signature,
        "fn foo(&str)"
    );
}

#[test]
fn batch_upserts_preserve_overloads_and_remove_only_stale_nodes() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let mut first = node("r", "src/lib.rs", "foo", 1);
    first.signature = "fn foo(i32)".into();
    let mut second = node("r", "src/lib.rs", "foo", 10);
    second.signature = "fn foo(&str)".into();
    let ids = storage
        .upsert_nodes(&[first.clone(), second.clone()])
        .unwrap();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);

    first.line_start = 30;
    first.line_end = 32;
    let retained = storage.upsert_nodes(&[first]).unwrap();
    storage
        .purge_file_nodes_except("r", "src/lib.rs", &retained)
        .unwrap();
    assert_eq!(storage.all_nodes().unwrap().len(), 1);
    assert_eq!(
        storage.get_node(retained[0]).unwrap().unwrap().line_start,
        30
    );
}

#[test]
fn upsert_edge_on_same_natural_key_updates_weight_not_row_count() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    let b = storage.upsert_node(&node("r", "b.rs", "b", 1)).unwrap();

    storage.upsert_edge(&edge(a, b, "CALLS_EXACT")).unwrap();
    storage
        .upsert_edge(&Edge {
            weight: 5.0,
            ..edge(a, b, "CALLS_EXACT")
        })
        .unwrap();

    let edges = storage.get_edges(a).unwrap();
    assert_eq!(
        edges.len(),
        1,
        "re-upserting the same edge must not duplicate it"
    );
    assert_eq!(edges[0].weight, 5.0);
}

#[test]
fn query_path_finds_shortest_route_across_multiple_hops() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    let b = storage.upsert_node(&node("r", "b.rs", "b", 1)).unwrap();
    let c = storage.upsert_node(&node("r", "c.rs", "c", 1)).unwrap();
    storage.upsert_edge(&edge(a, b, "CALLS_EXACT")).unwrap();
    storage.upsert_edge(&edge(b, c, "CALLS_EXACT")).unwrap();

    assert_eq!(storage.query_path(a, c).unwrap(), Some(vec![a, b, c]));
    assert_eq!(storage.query_path(a, a).unwrap(), Some(vec![a]));
}

#[test]
fn query_path_returns_none_when_unreachable() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    let b = storage.upsert_node(&node("r", "b.rs", "b", 1)).unwrap();

    assert_eq!(storage.query_path(a, b).unwrap(), None);
}

#[test]
fn upsert_contract_replaces_the_expectation_for_a_pair() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_contract("consumer", "provider", "hash1", "sha1", "blob1")
        .unwrap();
    storage
        .upsert_contract("consumer", "provider", "hash2", "sha2", "blob2")
        .unwrap();

    let expectations = storage.contract_expectations("consumer").unwrap();
    assert_eq!(expectations.len(), 1, "replace, never duplicate rows");
    assert_eq!(expectations[0].0, "provider");
    assert_eq!(expectations[0].1, "hash2");
    assert_eq!(expectations[0].2, "sha2");
    assert_eq!(expectations[0].3, "blob2");
}

#[test]
fn contract_expectations_are_scoped_to_the_consumer() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_contract("consumer_a", "provider", "hash_a", "sha1", "blob_a")
        .unwrap();
    storage
        .upsert_contract("consumer_b", "provider", "hash_b", "sha2", "blob_b")
        .unwrap();

    let for_a = storage.contract_expectations("consumer_a").unwrap();
    assert_eq!(for_a.len(), 1);
    assert_eq!(for_a[0].1, "hash_a");
}

#[test]
fn contract_expectations_is_empty_when_nothing_was_recorded() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    assert!(storage.contract_expectations("nobody").unwrap().is_empty());
}

#[test]
fn schema_version_reports_latest_after_open() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    assert_eq!(storage.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
}

#[test]
fn all_nodes_and_all_edges_list_everything_in_a_stable_order() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    let b = storage.upsert_node(&node("r", "b.rs", "b", 1)).unwrap();
    storage.upsert_edge(&edge(a, b, "CALLS_EXACT")).unwrap();

    let nodes = storage.all_nodes().unwrap();
    assert_eq!(nodes.iter().map(|n| n.id).collect::<Vec<_>>(), vec![a, b]);

    let edges = storage.all_edges().unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!((edges[0].source_id, edges[0].target_id), (a, b));
}

#[test]
fn purge_file_edges_removes_edges_in_both_directions() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "fn_a", 1)).unwrap();
    let b = storage.upsert_node(&node("r", "b.rs", "fn_b", 1)).unwrap();
    let c = storage.upsert_node(&node("r", "c.rs", "fn_c", 1)).unwrap();
    // a→b (outbound from a.rs), b→a (inbound to a.rs), c→b (unrelated)
    storage.upsert_edge(&edge(a, b, "CALLS_EXACT")).unwrap();
    storage.upsert_edge(&edge(b, a, "CALLS_EXACT")).unwrap();
    storage.upsert_edge(&edge(c, b, "CALLS_EXACT")).unwrap();

    let purged = storage.purge_file_edges("r", "a.rs").unwrap();
    // Both edges touching a.rs (a→b and b→a) must be gone; c→b survives.
    assert_eq!(purged, 2, "both directions must be purged");
    assert_eq!(storage.all_edges().unwrap().len(), 1);
    assert_eq!(storage.all_edges().unwrap()[0].source_id, c);
}

#[test]
fn purge_file_nodes_removes_only_the_target_file() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "fn_a", 1)).unwrap();
    storage.upsert_node(&node("r", "b.rs", "fn_b", 1)).unwrap();

    storage.purge_file_edges("r", "a.rs").unwrap();
    let purged = storage.purge_file_nodes("r", "a.rs").unwrap();

    assert_eq!(purged, 1);
    assert_eq!(storage.get_node(a).unwrap(), None, "a.rs node must be gone");
    assert_eq!(
        storage.all_nodes().unwrap().len(),
        1,
        "b.rs node must survive"
    );
}

#[test]
fn purge_file_edges_is_idempotent_on_already_clean_file() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage.upsert_node(&node("r", "a.rs", "fn_a", 1)).unwrap();

    let first = storage.purge_file_edges("r", "a.rs").unwrap();
    let second = storage.purge_file_edges("r", "a.rs").unwrap();
    assert_eq!(first, 0);
    assert_eq!(second, 0);
}

#[test]
fn query_path_does_not_loop_on_a_cycle() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    let b = storage.upsert_node(&node("r", "b.rs", "b", 1)).unwrap();
    let c = storage.upsert_node(&node("r", "c.rs", "c", 1)).unwrap();
    // a→b→c→a forms a cycle
    storage.upsert_edge(&edge(a, b, "CALLS_EXACT")).unwrap();
    storage.upsert_edge(&edge(b, c, "CALLS_EXACT")).unwrap();
    storage.upsert_edge(&edge(c, a, "CALLS_EXACT")).unwrap();

    // Must terminate (visited set prevents re-enqueuing), find direct path.
    assert_eq!(storage.query_path(a, c).unwrap(), Some(vec![a, b, c]));
    // c→a is unreachable via forward BFS from b in this graph
    assert_eq!(storage.query_path(b, a).unwrap(), Some(vec![b, c, a]));
}

fn journal_mode(path: &std::path::Path) -> String {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
        .unwrap()
}

#[test]
fn export_read_only_snapshot_writes_a_non_wal_copy() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("source.db");
    let mut storage = SqliteStorage::open(&source_path).unwrap();
    storage.upsert_node(&node("r", "a.rs", "fn_a", 1)).unwrap();
    assert_eq!(journal_mode(&source_path).to_lowercase(), "wal");

    let snapshot_path = dir.path().join("snapshot.idx");
    storage.export_read_only_snapshot(&snapshot_path).unwrap();

    assert_eq!(journal_mode(&snapshot_path).to_lowercase(), "delete");
}

#[test]
fn open_read_only_reads_an_exported_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("source.db");
    let mut storage = SqliteStorage::open(&source_path).unwrap();
    storage.upsert_node(&node("r", "a.rs", "fn_a", 1)).unwrap();

    let snapshot_path = dir.path().join("snapshot.idx");
    storage.export_read_only_snapshot(&snapshot_path).unwrap();

    let reader = SqliteStorage::open_read_only(&snapshot_path).unwrap();
    let nodes = reader.all_nodes().unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].symbol, "fn_a");
}

#[test]
fn open_read_only_refuses_a_schema_newer_than_this_binary_supports() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("future.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_version (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);
             INSERT INTO schema_version (version, applied_at) VALUES (999, 0);",
        )
        .unwrap();
    }
    match SqliteStorage::open_read_only(&path) {
        Err(weave_graph_core::StorageError::SchemaTooNew { found: 999, .. }) => {}
        Err(other) => panic!("expected SchemaTooNew{{found: 999}}, got {other:?}"),
        Ok(_) => panic!("expected SchemaTooNew{{found: 999}}, got Ok"),
    }
}

fn sample_note(target: Option<NodeId>, tier: NoteTier, expires_at: Option<i64>) -> Note {
    Note {
        id: 0,
        target_node_id: target,
        moniker: "a.rs#fn_a".into(),
        kind: "note".into(),
        tier,
        author: "agent".into(),
        content: "why this exists".into(),
        content_hash: Some("abc".into()),
        stale: false,
        expires_at,
        created_at: 1_000,
    }
}

#[test]
fn pinned_note_round_trips_through_recall() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    let id = storage
        .pin_note(&sample_note(Some(a), NoteTier::Crystallized, None))
        .unwrap();
    let recalled = storage.recall_notes(2_000).unwrap();
    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].id, id);
    assert_eq!(recalled[0].tier, NoteTier::Crystallized);
    assert_eq!(recalled[0].target_node_id, Some(a));
}

#[test]
fn recall_filters_expired_ephemerals_but_keeps_crystallized() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    // Ephemeral expired 100s before "now".
    storage
        .pin_note(&sample_note(Some(a), NoteTier::Ephemeral, Some(900)))
        .unwrap();
    // Ephemeral still live.
    storage
        .pin_note(&sample_note(Some(a), NoteTier::Ephemeral, Some(2_000)))
        .unwrap();
    // Crystallized never expires on its own.
    storage
        .pin_note(&sample_note(Some(a), NoteTier::Crystallized, None))
        .unwrap();

    assert_eq!(storage.recall_notes(1_000).unwrap().len(), 2);
    assert_eq!(
        storage.all_notes().unwrap().len(),
        3,
        "all_notes is the unfiltered view"
    );
}

#[test]
fn expired_ephemerals_are_deleted_only_on_demand() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    storage
        .pin_note(&sample_note(Some(a), NoteTier::Ephemeral, Some(900)))
        .unwrap();
    storage
        .pin_note(&sample_note(Some(a), NoteTier::Crystallized, None))
        .unwrap();
    // A crystallized note that somehow carries an expiry must not be
    // deleted by the sweep either — the tier, not the column, decides.
    storage
        .pin_note(&sample_note(Some(a), NoteTier::Crystallized, Some(100)))
        .unwrap();

    let deleted = storage.delete_expired_notes(1_000).unwrap();
    assert_eq!(deleted, 1);
    assert_eq!(storage.all_notes().unwrap().len(), 2);
}

#[test]
fn reattach_moves_the_target_and_sets_staleness() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    let b = storage.upsert_node(&node("r", "b.rs", "b", 1)).unwrap();
    let id = storage
        .pin_note(&sample_note(Some(a), NoteTier::Crystallized, None))
        .unwrap();
    // Purge-and-reinsert: same symbol, new node id, stale content.
    storage.reattach_note(id, Some(b), true).unwrap();
    let recalled = storage.recall_notes(1_000).unwrap();
    assert_eq!(recalled[0].target_node_id, Some(b));
    assert!(recalled[0].stale);

    // None target = orphaned, and recall still reports it.
    storage.reattach_note(id, None, false).unwrap();
    let recalled = storage.recall_notes(1_000).unwrap();
    assert_eq!(recalled[0].target_node_id, None);
    assert!(!recalled[0].stale);
}

#[test]
fn open_rebuild_creates_a_new_db_in_wal_mode() {
    let dir = tempfile::tempdir().unwrap();
    let rebuild_path = dir.path().join("graph.db.rebuild");
    let storage = SqliteStorage::open_rebuild(&rebuild_path).unwrap();
    assert_eq!(storage.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
}

/// The staging copy must carry un-checkpointed WAL content too — a raw
/// byte copy of the main file would silently drop pages still sitting in
/// the `-wal` sidecar and produce a stale rebuild database.
#[test]
fn backup_to_includes_uncheckpointed_wal_content() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("source.db");
    let mut storage = SqliteStorage::open(&source_path).unwrap();
    storage.upsert_node(&node("r", "a.rs", "fn_a", 1)).unwrap();

    let dest_path = dir.path().join("staged.db");
    storage.backup_to(&dest_path).unwrap();

    let staged = SqliteStorage::open_read_only(&dest_path).unwrap();
    let nodes = staged.all_nodes().unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].symbol, "fn_a");
}

#[test]
fn purge_orphaned_nodes_by_kind_removes_unreferenced_nodes_of_kind() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    let b = storage.upsert_node(&node("r", "b.rs", "b", 1)).unwrap();
    let c = storage.upsert_node(&node("r", "c.rs", "c", 1)).unwrap();

    // Connect 'b' to 'c'. 'a' and 'b' have no incoming edges.
    // 'c' has an incoming edge.
    // We cannot connect 'a' to 'c' and then delete 'a', because 'a' would have an outgoing edge, violating FK constraints on edges.source_id.
    // So 'a' is completely disconnected, 'b' has an outgoing edge, wait no! 'b' cannot have an outgoing edge either.
    // Let's connect 'c' to 'c' to give 'c' an incoming edge, and leave 'a' and 'b' disconnected.
    storage.upsert_edge(&edge(c, c, "CALLS")).unwrap();

    let removed = storage.purge_orphaned_nodes_by_kind("function").unwrap();
    assert_eq!(removed, 2);
    assert!(storage.get_node(a).unwrap().is_none());
    assert!(storage.get_node(b).unwrap().is_none());
    assert!(storage.get_node(c).unwrap().is_some());
}

#[test]
fn query_path_avoids_cycles_and_redundant_paths() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    let b = storage.upsert_node(&node("r", "b.rs", "b", 1)).unwrap();
    let c = storage.upsert_node(&node("r", "c.rs", "c", 1)).unwrap();

    // Diamond pattern: a -> b, a -> c, b -> c
    storage.upsert_edge(&edge(a, b, "CALLS")).unwrap();
    storage.upsert_edge(&edge(a, c, "CALLS")).unwrap();
    storage.upsert_edge(&edge(b, c, "CALLS")).unwrap();

    assert_eq!(storage.query_path(a, c).unwrap(), Some(vec![a, c]));
}

#[test]
fn checkpoint_wal_if_needed_is_safe_on_a_fresh_database() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    // No growth to react to yet — must not error just because there's
    // little or nothing for PASSIVE to checkpoint.
    storage.checkpoint_wal_if_needed().unwrap();
}

fn wal_size_after_writes(dir: &std::path::Path, name: &str, checkpoint_each_write: bool) -> u64 {
    let db_path = dir.join(name);
    let wal_path = dir.join(format!("{name}-wal"));
    let mut storage = SqliteStorage::open(&db_path).unwrap();
    for i in 0..500 {
        storage
            .upsert_node(&node("r", "a.rs", &format!("fn_{i}"), i))
            .unwrap();
        if checkpoint_each_write {
            storage.checkpoint_wal_if_needed().unwrap();
        }
    }
    std::fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0)
}

#[test]
fn checkpoint_wal_if_needed_keeps_the_wal_file_meaningfully_smaller() {
    let dir = tempfile::tempdir().unwrap();
    let with_valve = wal_size_after_writes(dir.path(), "with_valve.db", true);
    let without_valve = wal_size_after_writes(dir.path(), "without_valve.db", false);

    // PASSIVE checkpoints content (letting SQLite reuse WAL pages) but
    // doesn't shrink the file itself, so this isn't "near zero" — it's
    // "meaningfully bounded relative to never checkpointing at all",
    // which is the actual property the valve provides.
    assert!(
        with_valve < without_valve,
        "checkpointing every write ({with_valve} bytes) should leave a \
         smaller WAL than never checkpointing ({without_valve} bytes)"
    );
}

#[test]
fn get_callers_returns_edges_targeting_the_node() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    let b = storage.upsert_node(&node("r", "b.rs", "b", 1)).unwrap();
    let c = storage.upsert_node(&node("r", "c.rs", "c", 1)).unwrap();
    storage.upsert_edge(&edge(a, c, "CALLS")).unwrap();
    storage.upsert_edge(&edge(b, c, "CALLS")).unwrap();

    let callers = storage.get_callers(c).unwrap();

    assert_eq!(callers.len(), 2);
    assert!(callers.iter().all(|e| e.target_id == c));
    let sources: Vec<NodeId> = callers.iter().map(|e| e.source_id).collect();
    assert!(sources.contains(&a));
    assert!(sources.contains(&b));
}

#[test]
fn get_callers_returns_empty_for_a_node_with_no_incoming_edges() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();

    assert!(storage.get_callers(a).unwrap().is_empty());
}

#[test]
fn edge_count_reflects_upserts_and_purges() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    let b = storage.upsert_node(&node("r", "b.rs", "b", 1)).unwrap();
    assert_eq!(storage.edge_count().unwrap(), 0);

    storage.upsert_edge(&edge(a, b, "CALLS")).unwrap();
    assert_eq!(storage.edge_count().unwrap(), 1);

    storage.purge_file_edges("r", "a.rs").unwrap();
    assert_eq!(storage.edge_count().unwrap(), 0);
}

#[test]
fn upsert_and_purge_unresolved_refs_round_trip() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_unresolved_refs("r", "a.rs", &["helper".to_string()])
        .unwrap();

    let files = storage
        .get_files_with_unresolved_refs("r", "helper")
        .unwrap();
    assert_eq!(files, vec!["a.rs".to_string()]);

    let purged = storage.purge_file_unresolved_refs("r", "a.rs").unwrap();
    assert_eq!(purged, 1);
    assert!(
        storage
            .get_files_with_unresolved_refs("r", "helper")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn get_unresolved_refs_for_path_returns_only_that_files_refs() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_unresolved_refs("r", "a.rs", &["helper".to_string(), "widget".to_string()])
        .unwrap();
    storage
        .upsert_unresolved_refs("r", "b.rs", &["other".to_string()])
        .unwrap();

    let mut refs = storage.get_unresolved_refs_for_path("r", "a.rs").unwrap();
    refs.sort();
    assert_eq!(refs, vec!["helper".to_string(), "widget".to_string()]);
    assert!(
        storage
            .get_unresolved_refs_for_path("r", "nonexistent.rs")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn get_files_with_unresolved_refs_only_matches_the_given_short_name() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_unresolved_refs("r", "a.rs", &["helper".to_string()])
        .unwrap();
    storage
        .upsert_unresolved_refs("r", "b.rs", &["other".to_string()])
        .unwrap();

    assert_eq!(
        storage
            .get_files_with_unresolved_refs("r", "helper")
            .unwrap(),
        vec!["a.rs".to_string()]
    );
    assert_eq!(
        storage
            .get_files_with_unresolved_refs("r", "other")
            .unwrap(),
        vec!["b.rs".to_string()]
    );
}

#[test]
fn get_node_by_symbol_finds_an_exact_match() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    let b_id = storage.upsert_node(&node("r", "b.rs", "b", 1)).unwrap();

    let found = storage.get_node_by_symbol("b").unwrap().unwrap();
    assert_eq!(found.id, b_id);
    assert_eq!(found.path, "b.rs");
}

#[test]
fn get_node_by_symbol_returns_none_for_an_unknown_symbol() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();

    assert!(storage.get_node_by_symbol("ghost").unwrap().is_none());
}
