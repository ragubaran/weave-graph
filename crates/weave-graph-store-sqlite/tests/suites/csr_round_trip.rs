//! Parameterized CSR round-trip suite (see `suites/mod.rs`).

use std::path::Path;

use weave_graph_core::{CsrGraph, Edge, Node, Storage};

/// Constructor injection point: every suite fn opens storage through
/// this, so the same bodies run against any `Storage` backend.
pub type OpenFn = fn(&Path) -> Box<dyn Storage>;

fn node(path: &str, symbol: &str) -> Node {
    Node {
        id: 0,
        repo_id: "r".into(),
        path: path.into(),
        symbol: symbol.into(),
        kind: "function".into(),
        line_start: 1,
        line_end: 2,
        signature: String::new(),
    }
}

/// A `CsrGraph` loaded from SQL must answer path queries identically to
/// the SQL-backed BFS it was built from — the CSR is a derived read
/// structure, not an independent source of truth.
pub fn csr_query_path_matches_sql_query_path_on_the_same_graph(open: OpenFn) {
    let dir = tempfile::tempdir().unwrap();
    let mut storage = open(&dir.path().join("graph.db"));

    let a = storage.upsert_node(&node("a.rs", "a")).unwrap();
    let b = storage.upsert_node(&node("b.rs", "b")).unwrap();
    let c = storage.upsert_node(&node("c.rs", "c")).unwrap();
    let d = storage.upsert_node(&node("d.rs", "d")).unwrap(); // unreachable island
    storage
        .upsert_edge(&Edge {
            id: 0,
            source_id: a,
            target_id: b,
            kind: "CALLS_EXACT".into(),
            weight: 1.0,
            extractor: None,
            resolution_kind: None,
        })
        .unwrap();
    storage
        .upsert_edge(&Edge {
            id: 0,
            source_id: b,
            target_id: c,
            kind: "CALLS_EXACT".into(),
            weight: 1.0,
            extractor: None,
            resolution_kind: None,
        })
        .unwrap();

    let csr = CsrGraph::load(&*storage).unwrap();

    for (from, to) in [(a, c), (a, a), (c, a), (a, d), (b, a)] {
        assert_eq!(
            csr.query_path(from, to),
            storage.query_path(from, to).unwrap(),
            "CsrGraph and the storage backend disagree on path from {from} to {to}"
        );
    }
}
