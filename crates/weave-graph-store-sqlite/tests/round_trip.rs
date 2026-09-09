use weave_graph_core::{Edge, Node, Storage};
use weave_graph_store_sqlite::SqliteStorage;

fn node(repo: &str, path: &str, symbol: &str) -> Node {
    Node {
        id: 0,
        repo_id: repo.into(),
        path: path.into(),
        symbol: symbol.into(),
        kind: "function".into(),
        line_start: 10,
        line_end: 20,
        signature: format!("fn {symbol}()"),
    }
}

/// `impl.md` M1.1's required round-trip: write a graph, close the
/// database, reopen it from the same path, and read back an identical
/// graph — proving persistence survives a real process-boundary close,
/// not just an in-memory connection.
#[test]
fn graph_survives_close_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("graph.db");

    let (node_a_id, node_b_id, edge_id) = {
        let mut storage = SqliteStorage::open(&db_path).unwrap();
        let a = storage.upsert_node(&node("r", "a.rs", "a")).unwrap();
        let b = storage.upsert_node(&node("r", "b.rs", "b")).unwrap();
        let edge_id = storage
            .upsert_edge(&Edge {
                id: 0,
                source_id: a,
                target_id: b,
                kind: "CALLS_EXACT".into(),
                weight: 1.0,
            })
            .unwrap();
        (a, b, edge_id)
    }; // `storage` dropped here — connection closed.

    let reopened = SqliteStorage::open(&db_path).unwrap();

    let a = reopened.get_node(node_a_id).unwrap().unwrap();
    assert_eq!(a.symbol, "a");
    assert_eq!(a.path, "a.rs");

    let b = reopened.get_node(node_b_id).unwrap().unwrap();
    assert_eq!(b.symbol, "b");

    let edges = reopened.get_edges(node_a_id).unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].id, edge_id);
    assert_eq!(edges[0].target_id, node_b_id);
    assert_eq!(edges[0].kind, "CALLS_EXACT");

    assert_eq!(reopened.schema_version().unwrap(), 3);
}
