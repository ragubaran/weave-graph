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
