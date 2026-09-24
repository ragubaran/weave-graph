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
        extractor: None,
        resolution_kind: None,
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
            precise_only: false,
        },
        None,
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

/// P10.5: a heuristically-resolved incoming caller is marked `[heuristic]`
/// in `weave_trace_calls`'s own text — the one side of this tool with a
/// real per-edge kind to classify (`outgoing_chain`/`weave_impact_radius`
/// walk the CSR, which never carries edge kind, so they can't).
#[test]
fn a_heuristically_resolved_caller_is_marked_in_the_incoming_chain() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let exact_caller = storage.upsert_node(&node("exactCaller")).unwrap();
    let heuristic_caller = storage.upsert_node(&node("heuristicCaller")).unwrap();
    let root = storage.upsert_node(&node("root")).unwrap();
    storage.upsert_edge(&edge(exact_caller, root)).unwrap();
    storage
        .upsert_edge(&Edge {
            kind: "CALLS_DYNAMIC".into(),
            resolution_kind: Some(weave_graph_core::AMBIGUOUS_HEURISTIC.to_string()),
            ..edge(heuristic_caller, root)
        })
        .unwrap();

    let csr = CsrGraph::load(&storage).unwrap();
    let result = weave_trace_calls(
        &storage,
        &csr,
        TraceCallsArgs {
            symbol: "root",
            depth: 1,
            max_tokens: None,
            precise_only: false,
        },
        None,
    );
    assert!(
        result
            .text
            .contains("heuristicCaller (heuristicCaller.rs:1) [heuristic]"),
        "{}",
        result.text
    );
    assert!(
        result.text.contains("exactCaller (exactCaller.rs:1)\n")
            || result.text.contains("exactCaller (exactCaller.rs:1)"),
        "{}",
        result.text
    );
    assert!(
        !result
            .text
            .lines()
            .any(|l| l.contains("exactCaller") && l.contains("[heuristic]")),
        "the exact caller must not be marked heuristic: {}",
        result.text
    );
}

#[test]
fn precise_only_drops_the_heuristic_caller_but_keeps_traversal_past_it() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let exact_caller = storage.upsert_node(&node("exactCaller")).unwrap();
    let heuristic_caller = storage.upsert_node(&node("heuristicCaller")).unwrap();
    let root = storage.upsert_node(&node("root")).unwrap();
    let grandparent = storage.upsert_node(&node("grandparent")).unwrap();
    storage.upsert_edge(&edge(exact_caller, root)).unwrap();
    storage
        .upsert_edge(&Edge {
            kind: "CALLS_DYNAMIC".into(),
            resolution_kind: Some(weave_graph_core::AMBIGUOUS_HEURISTIC.to_string()),
            ..edge(heuristic_caller, root)
        })
        .unwrap();
    // The heuristic caller's own caller — must still surface even though
    // the heuristic caller itself is dropped from display (filtering
    // narrows what's *shown*, never what's *traversed*).
    storage
        .upsert_edge(&edge(grandparent, heuristic_caller))
        .unwrap();

    let csr = CsrGraph::load(&storage).unwrap();
    let result = weave_trace_calls(
        &storage,
        &csr,
        TraceCallsArgs {
            symbol: "root",
            depth: 2,
            max_tokens: None,
            precise_only: true,
        },
        None,
    );
    assert!(result.text.contains("exactCaller ("), "{}", result.text);
    assert!(
        !result.text.contains("heuristicCaller ("),
        "{}",
        result.text
    );
    assert!(
        result.text.contains("grandparent ("),
        "traversal must continue through a filtered-out node: {}",
        result.text
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
            precise_only: false,
        },
        None,
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
            precise_only: false,
        },
        None,
    );
    assert!(result.text.contains("trace_calls: a"));
}

// ─── token-budgeted chain truncation ─────────────────────────

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
                extractor: None,
                resolution_kind: None,
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
            precise_only: false,
        },
        None,
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
            precise_only: false,
        },
        None,
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
