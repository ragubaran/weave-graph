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
    let mut storage = TursoStorage::open_in_memory().unwrap();
    let id = storage
        .upsert_node(&node("r", "src/lib.rs", "foo", 1))
        .unwrap();
    let got = storage.get_node(id).unwrap().unwrap();
    assert_eq!(got.symbol, "foo");
    assert_eq!(got.line_start, 1);
    assert!(storage.get_node(id + 1).unwrap().is_none());
}

#[test]
fn upsert_node_by_natural_key_updates_in_place() {
    let mut storage = TursoStorage::open_in_memory().unwrap();
    let first = storage.upsert_node(&node("r", "a.rs", "f", 1)).unwrap();
    let second = storage.upsert_node(&node("r", "a.rs", "f", 1)).unwrap();
    assert_eq!(first, second);
    assert_eq!(storage.all_nodes().unwrap().len(), 1);
}

#[test]
fn edges_are_visible_in_both_directions() {
    let mut storage = TursoStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    let b = storage.upsert_node(&node("r", "b.rs", "b", 1)).unwrap();
    storage.upsert_edge(&edge(a, b, "CALLS_EXACT")).unwrap();

    assert_eq!(storage.get_edges(a).unwrap().len(), 1);
    assert_eq!(storage.get_callers(b).unwrap().len(), 1);
    assert_eq!(storage.query_path(a, b).unwrap(), Some(vec![a, b]));
}

#[test]
fn file_backed_storage_survives_close_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.db");
    let id = {
        let mut storage = TursoStorage::open(&path).unwrap();
        let id = storage.upsert_node(&node("r", "a.rs", "f", 1)).unwrap();
        assert_eq!(storage.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
        id
    };
    let reopened = TursoStorage::open(&path).unwrap();
    assert!(reopened.get_node(id).unwrap().is_some());
}

#[test]
fn open_refuses_a_schema_newer_than_supported() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.db");
    TursoStorage::open(&path).unwrap();

    // Simulate a database written by a newer binary.
    let db = futures::executor::block_on(libsql::Builder::new_local(&path).build()).unwrap();
    let conn = db.connect().unwrap();
    futures::executor::block_on(conn.execute(
        "INSERT INTO schema_version (version, applied_at) VALUES (99, 0)",
        (),
    ))
    .unwrap();

    match TursoStorage::open(&path) {
        Err(StorageError::SchemaTooNew { found, max }) => {
            assert_eq!((found, max), (99, LATEST_SCHEMA_VERSION));
        }
        Err(e) => panic!("expected SchemaTooNew, got {e:?}"),
        Ok(_) => panic!("expected SchemaTooNew, got Ok"),
    }
}

#[test]
fn purge_file_edges_removes_both_directions() {
    let mut storage = TursoStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("repo", "a.rs", "fa", 1)).unwrap();
    let b = storage.upsert_node(&node("repo", "b.rs", "fb", 1)).unwrap();
    let c = storage.upsert_node(&node("repo", "c.rs", "fc", 1)).unwrap();
    storage.upsert_edge(&edge(a, b, "CALLS_EXACT")).unwrap();
    storage.upsert_edge(&edge(b, a, "CALLS_EXACT")).unwrap();
    storage.upsert_edge(&edge(c, b, "CALLS_EXACT")).unwrap();

    let purged = storage.purge_file_edges("repo", "a.rs").unwrap();
    assert_eq!(purged, 2);
    assert_eq!(storage.all_edges().unwrap().len(), 1);
}

#[test]
fn query_path_finds_shortest_route_across_multiple_hops() {
    let mut storage = TursoStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    let b = storage.upsert_node(&node("r", "b.rs", "b", 1)).unwrap();
    let c = storage.upsert_node(&node("r", "c.rs", "c", 1)).unwrap();
    storage.upsert_edge(&edge(a, b, "CALLS_EXACT")).unwrap();
    storage.upsert_edge(&edge(b, c, "CALLS_EXACT")).unwrap();

    assert_eq!(storage.query_path(a, c).unwrap(), Some(vec![a, b, c]));
    assert_eq!(storage.query_path(a, a).unwrap(), Some(vec![a]));
}

#[test]
fn query_path_does_not_loop_on_a_cycle() {
    let mut storage = TursoStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    let b = storage.upsert_node(&node("r", "b.rs", "b", 1)).unwrap();
    let c = storage.upsert_node(&node("r", "c.rs", "c", 1)).unwrap();
    // a→b→c→a forms a cycle
    storage.upsert_edge(&edge(a, b, "CALLS_EXACT")).unwrap();
    storage.upsert_edge(&edge(b, c, "CALLS_EXACT")).unwrap();
    storage.upsert_edge(&edge(c, a, "CALLS_EXACT")).unwrap();

    // Must terminate (visited set prevents re-enqueuing the cycle's `a`).
    assert_eq!(storage.query_path(a, c).unwrap(), Some(vec![a, b, c]));
    assert_eq!(storage.query_path(b, a).unwrap(), Some(vec![b, c, a]));
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
    let mut storage = TursoStorage::open_in_memory().unwrap();
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
    let mut storage = TursoStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    storage
        .pin_note(&sample_note(Some(a), NoteTier::Ephemeral, Some(900)))
        .unwrap();
    storage
        .pin_note(&sample_note(Some(a), NoteTier::Ephemeral, Some(2_000)))
        .unwrap();
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
    let mut storage = TursoStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    storage
        .pin_note(&sample_note(Some(a), NoteTier::Ephemeral, Some(900)))
        .unwrap();
    storage
        .pin_note(&sample_note(Some(a), NoteTier::Crystallized, None))
        .unwrap();
    storage
        .pin_note(&sample_note(Some(a), NoteTier::Crystallized, Some(100)))
        .unwrap();

    let deleted = storage.delete_expired_notes(1_000).unwrap();
    assert_eq!(deleted, 1);
    assert_eq!(storage.all_notes().unwrap().len(), 2);
}

#[test]
fn reattach_moves_the_target_and_sets_staleness() {
    let mut storage = TursoStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    let b = storage.upsert_node(&node("r", "b.rs", "b", 1)).unwrap();
    let id = storage
        .pin_note(&sample_note(Some(a), NoteTier::Crystallized, None))
        .unwrap();
    storage.reattach_note(id, Some(b), true).unwrap();
    let recalled = storage.recall_notes(1_000).unwrap();
    assert_eq!(recalled[0].target_node_id, Some(b));
    assert!(recalled[0].stale);

    storage.reattach_note(id, None, false).unwrap();
    let recalled = storage.recall_notes(1_000).unwrap();
    assert_eq!(recalled[0].target_node_id, None);
    assert!(!recalled[0].stale);
}

#[test]
fn open_on_a_corrupt_file_surfaces_a_backend_error() {
    // Not a valid SQLite/libSQL file at all — the connect step itself
    // must fail and flow through `backend_err`, not panic.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.db");
    std::fs::write(&path, b"not a real database file").unwrap();

    match TursoStorage::open(&path) {
        Err(StorageError::Backend(_)) => {}
        Err(e) => panic!("expected a Backend error, got {e:?}"),
        Ok(_) => panic!("expected a Backend error, got Ok"),
    }
}

#[test]
fn backend_err_mapping_on_invalid_query() {
    let storage = TursoStorage::open_in_memory().unwrap();
    // Drop a table to force a query error during a normal operation.
    futures::executor::block_on(storage.conn.execute("DROP TABLE nodes", ())).unwrap();
    let res = storage.get_node(1);
    assert!(matches!(res, Err(StorageError::Backend(_))));
}

#[test]
fn query_path_avoids_cycles_and_redundant_paths() {
    let mut storage = TursoStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("r", "a.rs", "a", 1)).unwrap();
    let b = storage.upsert_node(&node("r", "b.rs", "b", 1)).unwrap();
    let c = storage.upsert_node(&node("r", "c.rs", "c", 1)).unwrap();

    storage.upsert_edge(&edge(a, b, "CALLS")).unwrap();
    storage.upsert_edge(&edge(a, c, "CALLS")).unwrap();
    storage.upsert_edge(&edge(b, c, "CALLS")).unwrap();

    assert_eq!(storage.query_path(a, c).unwrap(), Some(vec![a, c]));
}
