use super::*;
use crate::tools::TraceCallsArgs;
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
fn trace_calls_finds_outgoing_and_incoming() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let id1 = storage.upsert_node(&node("caller")).unwrap();
    let id2 = storage.upsert_node(&node("root")).unwrap();
    let id3 = storage.upsert_node(&node("callee")).unwrap();
    storage.upsert_edge(&edge(id1, id2)).unwrap();
    storage.upsert_edge(&edge(id2, id3)).unwrap();

    let csr = CsrGraph::load(&storage).unwrap();
    let result = weave_trace_calls(
        &storage,
        &csr,
        TraceCallsArgs {
            symbol: "root",
            depth: 2,
            max_tokens: None,
        },
    );
    assert!(
        result.text.contains("callee"),
        "outgoing must include callee"
    );
    assert!(
        result.text.contains("caller"),
        "incoming must include caller"
    );
}

#[test]
fn unknown_symbol_returns_not_found_message() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let csr = CsrGraph::load(&storage).unwrap();
    let result = weave_trace_calls(
        &storage,
        &csr,
        TraceCallsArgs {
            symbol: "ghost",
            depth: 3,
            max_tokens: None,
        },
    );
    assert!(result.text.contains("not found"));
}

#[test]
fn trace_calls_terminates_on_cycle() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let id1 = storage.upsert_node(&node("a")).unwrap();
    let id2 = storage.upsert_node(&node("b")).unwrap();
    storage.upsert_edge(&edge(id1, id2)).unwrap();
    storage.upsert_edge(&edge(id2, id1)).unwrap();

    let csr = CsrGraph::load(&storage).unwrap();
    let result = weave_trace_calls(
        &storage,
        &csr,
        TraceCallsArgs {
            symbol: "a",
            depth: 10,
            max_tokens: None,
        },
    );
    assert!(result.text.contains("trace_calls: a"));
}

// ─── impl.md M2.16: token-budgeted chain truncation ─────────────────────────

#[test]
fn small_max_tokens_truncates_chains_with_explicit_counts() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    // A hub with a long outgoing chain from `root`.
    let root = storage
        .upsert_node(&Node {
            id: 1,
            repo_id: "r".into(),
            path: "hub.rs".into(),
            symbol: "root".into(),
            kind: "function".into(),
            line_start: 1,
            line_end: 2,
            signature: String::new(),
        })
        .unwrap();
    for i in 0..40 {
        let leaf = storage
            .upsert_node(&Node {
                id: 10 + i,
                repo_id: "r".into(),
                path: format!("leaf{i}.rs"),
                symbol: format!("leaf{i:02}_with_a_long_descriptive_name"),
                kind: "function".into(),
                line_start: 1,
                line_end: 2,
                signature: String::new(),
            })
            .unwrap();
        storage
            .upsert_edge(&weave_graph_core::Edge {
                id: 0,
                source_id: root,
                target_id: leaf,
                kind: "CALLS_EXACT".into(),
                weight: 1.0,
            })
            .unwrap();
    }
    let csr = CsrGraph::load(&storage).unwrap();

    let shed = weave_trace_calls(
        &storage,
        &csr,
        TraceCallsArgs {
            symbol: "root",
            depth: 1,
            max_tokens: Some(20),
        },
    );
    assert!(
        crate::tools::estimate_tokens(&shed.text) <= 20,
        "{}",
        shed.text
    );
    assert!(
        shed.text.contains("… and"),
        "explicit truncation marker: {}",
        shed.text
    );
    assert!(
        shed.text.contains("outgoing (40):"),
        "totals never go silent: {}",
        shed.text
    );

    // No budget: full chain, byte-identical legacy format.
    let full = weave_trace_calls(
        &storage,
        &csr,
        TraceCallsArgs {
            symbol: "root",
            depth: 1,
            max_tokens: None,
        },
    );
    assert!(
        full.text
            .contains("leaf00_with_a_long_descriptive_name (leaf0.rs:1)")
    );
    assert!(
        full.text
            .contains("leaf39_with_a_long_descriptive_name (leaf39.rs:1)")
    );
}
