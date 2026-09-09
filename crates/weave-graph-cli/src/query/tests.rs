use weave_graph_core::{Edge, Node};
use weave_graph_store_sqlite::SqliteStorage;

use super::*;

fn node(path: &str, symbol: &str) -> Node {
    Node {
        id: 0,
        repo_id: "r".into(),
        path: path.into(),
        symbol: symbol.into(),
        kind: "function".into(),
        line_start: 1,
        line_end: 3,
        signature: format!("fn {symbol}()"),
    }
}

fn edge(source_id: NodeId, target_id: NodeId) -> Edge {
    Edge {
        id: 0,
        source_id,
        target_id,
        kind: "CALLS_EXACT".into(),
        weight: 1.0,
    }
}

// caller -> a -> b -> c (chain)
fn chain_storage() -> SqliteStorage {
    let mut s = SqliteStorage::open_in_memory().unwrap();
    let caller = s.upsert_node(&node("caller.rs", "caller")).unwrap();
    let a = s.upsert_node(&node("a.rs", "a")).unwrap();
    let b = s.upsert_node(&node("b.rs", "b")).unwrap();
    let c = s.upsert_node(&node("c.rs", "c")).unwrap();
    s.upsert_edge(&edge(caller, a)).unwrap();
    s.upsert_edge(&edge(a, b)).unwrap();
    s.upsert_edge(&edge(b, c)).unwrap();
    s
}

#[test]
fn callers_finds_transitive_callers() {
    let storage = chain_storage();
    let result = run(&storage, "callers(b)").unwrap();
    assert!(result.contains("a ("));
    assert!(result.contains("caller ("));
    assert!(!result.contains("c ("));
}

#[test]
fn callees_is_bounded_to_direct_neighbors_only() {
    let storage = chain_storage();
    let result = run(&storage, "callees(a)").unwrap();
    assert!(result.contains("b ("));
    assert!(
        !result.contains("c ("),
        "callees(a) must not include transitive c"
    );
}

#[test]
fn impact_is_unbounded_transitively() {
    let storage = chain_storage();
    let result = run(&storage, "impact(a)").unwrap();
    assert!(result.contains("b ("));
    assert!(
        result.contains("c ("),
        "impact(a) must include transitive c"
    );
}

#[test]
fn path_finds_the_shortest_chain() {
    let storage = chain_storage();
    let result = run(&storage, "path(caller,c)").unwrap();
    assert_eq!(result, "caller → a → b → c");
}

#[test]
fn path_reports_no_path_found_when_unreachable() {
    let storage = chain_storage();
    let result = run(&storage, "path(c,caller)").unwrap();
    assert_eq!(result, "no path found");
}

#[test]
fn unresolvable_symbol_is_a_clear_error() {
    let storage = chain_storage();
    let err = run(&storage, "callers(nope)").unwrap_err();
    assert!(err.contains("symbol not found: nope"));
}

#[test]
fn malformed_expression_is_a_clear_error() {
    let storage = chain_storage();
    assert!(run(&storage, "not a call").is_err());
}

#[test]
fn wrong_argument_count_is_a_clear_error() {
    let storage = chain_storage();
    assert!(run(&storage, "callers(a,b)").is_err());
    assert!(run(&storage, "path(a)").is_err());
}

#[test]
fn unknown_function_is_a_clear_error() {
    let storage = chain_storage();
    let err = run(&storage, "bogus(a)").unwrap_err();
    assert!(err.contains("unknown query function"));
}
