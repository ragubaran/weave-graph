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
