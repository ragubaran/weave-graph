use weave_graph_core::{Node, Storage};

use crate::SqliteStorage;

fn node(symbol: &str, signature: &str) -> Node {
    Node {
        id: 0,
        repo_id: "local".to_string(),
        path: "src/lib.rs".to_string(),
        symbol: symbol.to_string(),
        kind: "function".to_string(),
        line_start: 1,
        line_end: 2,
        signature: signature.to_string(),
    }
}

#[test]
fn rebuild_then_search_finds_a_split_camel_case_identifier() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_node(&node("checkJwtTtl", "fn checkJwtTtl()"))
        .unwrap();
    storage
        .upsert_node(&node("unrelatedHelper", "fn unrelatedHelper()"))
        .unwrap();
    storage.rebuild_fts_index().unwrap();

    let expanded = weave_graph_core::synonym::expand_query("token lifetime");
    let hits = storage.search_symbols(&expanded, 10).unwrap();

    assert_eq!(hits.len(), 1);
    let hit = storage.get_node(hits[0]).unwrap().unwrap();
    assert_eq!(hit.symbol, "checkJwtTtl");
}

#[test]
fn rebuild_clears_stale_entries_after_the_node_is_purged() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_node(&node("authLogin", "fn authLogin()"))
        .unwrap();
    storage.rebuild_fts_index().unwrap();
    assert_eq!(storage.search_symbols("\"login\"", 10).unwrap().len(), 1);

    // A rebuild after the underlying node is gone must not still surface
    // it — `symbol_fts` is a derived index, never its own source of truth.
    storage.purge_file_nodes("local", "src/lib.rs").unwrap();
    storage.rebuild_fts_index().unwrap();

    assert!(storage.search_symbols("\"login\"", 10).unwrap().is_empty());
}

#[test]
fn search_respects_the_limit() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    for i in 0..5 {
        storage
            .upsert_node(&node(&format!("authHandler{i}"), "fn f()"))
            .unwrap();
    }
    storage.rebuild_fts_index().unwrap();

    let hits = storage.search_symbols("\"auth\"", 2).unwrap();
    assert_eq!(hits.len(), 2);
}

#[test]
fn search_with_no_match_returns_empty() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_node(&node("authLogin", "fn authLogin()"))
        .unwrap();
    storage.rebuild_fts_index().unwrap();

    assert!(
        storage
            .search_symbols("\"nonexistentterm\"", 10)
            .unwrap()
            .is_empty()
    );
}
