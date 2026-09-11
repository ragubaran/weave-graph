use super::*;

use pyo3::Python;
use tempfile::tempdir;

/// Tests embed the interpreter via `prepare_freethreaded_python` — no
/// `auto-initialize` feature needed (it can't coexist with wheels).
fn init_python() {
    pyo3::prepare_freethreaded_python();
}
use weave_graph_core::{Edge, Node, Storage};
use weave_graph_store_sqlite::SqliteStorage;

fn node(path: &str, symbol: &str, line: u32) -> Node {
    Node {
        id: 0,
        repo_id: "r".into(),
        path: path.into(),
        symbol: symbol.into(),
        kind: "function".into(),
        line_start: line,
        line_end: line + 2,
        signature: format!("fn {symbol}()"),
    }
}

fn edge(source_id: u32, target_id: u32) -> Edge {
    Edge {
        id: 0,
        source_id,
        target_id,
        kind: "CALLS_EXACT".into(),
        weight: 1.0,
    }
}

/// a.rs caller -> b.rs callee; c.rs orphan (no edges).
fn seed_db(path: &std::path::Path) -> (u32, u32, u32) {
    let mut storage = SqliteStorage::open(path).unwrap();
    let a = storage.upsert_node(&node("a.rs", "caller", 1)).unwrap();
    let b = storage.upsert_node(&node("b.rs", "callee", 10)).unwrap();
    let c = storage.upsert_node(&node("c.rs", "orphan", 20)).unwrap();
    storage.upsert_edge(&edge(a, b)).unwrap();
    (a, b, c)
}

fn open_graph(path: &std::path::Path) -> WeaveGraph {
    init_python();
    Python::with_gil(|_| WeaveGraph::new(path.to_str().unwrap()).unwrap())
}

#[test]
fn get_node_returns_the_full_record_and_none_for_unknown_ids() {
    init_python();
    let dir = tempdir().unwrap();
    let path = dir.path().join("graph.db");
    let (a, _b, _c) = seed_db(&path);
    let graph = open_graph(&path);

    Python::with_gil(|py| {
        let node = graph.get_node(py, a).unwrap().unwrap();
        assert_eq!(
            node.get_item("symbol")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "caller"
        );
        assert_eq!(
            node.get_item("path")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "a.rs"
        );
        assert!(graph.get_node(py, 9999).unwrap().is_none());
    });
}

#[test]
fn get_edges_query_path_and_radius_round_trip() {
    init_python();
    let dir = tempdir().unwrap();
    let path = dir.path().join("graph.db");
    let (a, b, c) = seed_db(&path);
    let graph = open_graph(&path);

    Python::with_gil(|py| {
        let edges = graph.get_edges(py, a).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(
            edges[0]
                .get_item("kind")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "CALLS_EXACT"
        );

        assert_eq!(graph.query_path(a, b).unwrap(), Some(vec![a, b]));
        assert_eq!(graph.query_path(a, c).unwrap(), None);

        let radius = graph.impact_radius("caller").unwrap();
        assert_eq!(radius.len(), 1);
        assert_eq!(radius[0].0, "callee");
    });
}

#[test]
fn trace_calls_returns_both_directions() {
    init_python();
    let dir = tempdir().unwrap();
    let path = dir.path().join("graph.db");
    let (a, b, _c) = seed_db(&path);
    let mut storage = SqliteStorage::open(&path).unwrap();
    // Make the call graph mutual so the incoming direction has data.
    storage.upsert_edge(&edge(b, a)).unwrap();
    let graph = open_graph(&path);

    let (outgoing, incoming) = graph.trace_calls("caller", 2).unwrap();
    assert_eq!(outgoing.len(), 1);
    assert!(outgoing[0].starts_with("callee (b.rs:10)"));
    assert_eq!(incoming.len(), 1);
    assert!(incoming[0].starts_with("callee (b.rs:10)"));
}

#[test]
fn unknown_symbol_is_a_clear_error() {
    init_python();
    let dir = tempdir().unwrap();
    let path = dir.path().join("graph.db");
    seed_db(&path);
    let graph = open_graph(&path);

    Python::with_gil(|_py| {
        assert!(graph.impact_radius("nope").is_err());
        assert!(graph.trace_calls("nope", 2).is_err());
    });
}

#[test]
fn schema_version_helper_reports_latest() {
    init_python();
    let dir = tempdir().unwrap();
    let path = dir.path().join("graph.db");
    seed_db(&path);
    assert_eq!(
        schema_version(path.to_str().unwrap()).unwrap(),
        weave_graph_core::schema::LATEST_SCHEMA_VERSION
    );
}
