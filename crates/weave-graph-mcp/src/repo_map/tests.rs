use super::*;
use crate::tools::RepoMapArgs;
use weave_graph_core::{Edge, Node, Storage};
use weave_graph_store_sqlite::SqliteStorage;

fn node(path: &str, symbol: &str) -> Node {
    Node {
        id: 0,
        repo_id: "r".into(),
        path: path.into(),
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
fn repo_map_ranks_by_degree_and_respects_max_files() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let id1 = storage.upsert_node(&node("hub.rs", "hub")).unwrap();
    let id2 = storage.upsert_node(&node("leaf.rs", "leaf")).unwrap();
    let id3 = storage.upsert_node(&node("hub.rs", "hub2")).unwrap();
    storage.upsert_edge(&edge(id1, id2)).unwrap();
    storage.upsert_edge(&edge(id3, id2)).unwrap();

    let csr = CsrGraph::load(&storage).unwrap();
    let result = weave_repo_map(&storage, &csr, RepoMapArgs { max_files: 10 });
    assert!(result.text.contains("hub.rs"), "hub.rs must appear");
    assert!(
        result.text.find("hub.rs") < result.text.find("leaf.rs"),
        "hub before leaf"
    );
}

#[test]
fn repo_map_truncates_to_max_files() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    for i in 1..=5 {
        storage
            .upsert_node(&node(&format!("f{i}.rs"), &format!("s{i}")))
            .unwrap();
    }
    let csr = CsrGraph::load(&storage).unwrap();
    let result = weave_repo_map(&storage, &csr, RepoMapArgs { max_files: 2 });
    let file_lines = result
        .text
        .lines()
        .filter(|l| l.trim_start().starts_with('f'))
        .count();
    assert_eq!(file_lines, 2);
}
