use super::*;
use crate::tools::ImpactRadiusArgs;
use weave_graph_core::{Edge, Node, Storage};
use weave_graph_store_sqlite::SqliteStorage;

fn node(symbol: &str) -> Node {
    Node {
        id: 0,
        repo_id: "r".into(),
        path: format!("{symbol}.rs"),
        symbol: symbol.into(),
        kind: "function".into(),
        line_start: 1,
        line_end: 5,
        signature: format!("fn {symbol}()"),
    }
}
fn edge(src: u32, tgt: u32) -> Edge {
    Edge {
        id: 0,
        source_id: src,
        target_id: tgt,
        kind: "CALLS_EXACT".into(),
        weight: 1.0,
    }
}

#[test]
fn impact_radius_finds_all_downstream_nodes() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let id1 = storage.upsert_node(&node("root")).unwrap();
    let id2 = storage.upsert_node(&node("b")).unwrap();
    let id3 = storage.upsert_node(&node("c")).unwrap();
    let id4 = storage.upsert_node(&node("d")).unwrap();
    storage.upsert_edge(&edge(id1, id2)).unwrap();
    storage.upsert_edge(&edge(id2, id3)).unwrap();
    storage.upsert_edge(&edge(id3, id4)).unwrap();

    let csr = CsrGraph::load(&storage).unwrap();
    let result = weave_impact_radius(&storage, &csr, ImpactRadiusArgs { symbol: "root" });
    assert_eq!(result.symbol_count, 3, "b, c, d all impacted");
    assert!(result.text.contains("3 symbols affected"));
}

#[test]
fn impact_radius_terminates_on_cycle() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let id1 = storage.upsert_node(&node("a")).unwrap();
    let id2 = storage.upsert_node(&node("b")).unwrap();
    storage.upsert_edge(&edge(id1, id2)).unwrap();
    storage.upsert_edge(&edge(id2, id1)).unwrap();

    let csr = CsrGraph::load(&storage).unwrap();
    let result = weave_impact_radius(&storage, &csr, ImpactRadiusArgs { symbol: "a" });
    assert_eq!(result.symbol_count, 1, "only b is impacted, no duplicate a");
}

#[test]
fn unknown_symbol_returns_not_found() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let csr = CsrGraph::load(&storage).unwrap();
    let result = weave_impact_radius(&storage, &csr, ImpactRadiusArgs { symbol: "ghost" });
    assert!(result.text.contains("not found"));
    assert_eq!(result.symbol_count, 0);
}

#[test]
fn large_radius_truncates_display_to_20() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let root = storage.upsert_node(&node("s1")).unwrap();
    for i in 2..=22 {
        let target = storage.upsert_node(&node(&format!("s{i}"))).unwrap();
        storage.upsert_edge(&edge(root, target)).unwrap();
    }
    let csr = CsrGraph::load(&storage).unwrap();
    let result = weave_impact_radius(&storage, &csr, ImpactRadiusArgs { symbol: "s1" });
    assert_eq!(result.symbol_count, 21);
    assert!(result.text.contains("... and 1 more"));
}
