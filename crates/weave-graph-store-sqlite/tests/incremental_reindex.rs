//! M1.4 integration tests — incremental reindex correctness.
//!
//! The test `reindex_one_of_two_mutually_referencing_files_leaves_zero_dangling_edges`
//! is the **blocking regression test** named in `plan.md` §1.2a.
//! It must pass before any M1.4 change merges.

use std::path::PathBuf;

use weave_graph_core::{Edge, Node, NodeId, ReindexConfig, Storage, should_bail_out};
use weave_graph_store_sqlite::SqliteStorage;

fn tmp_db() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    (dir, path)
}

fn node(path: &str, symbol: &str, line: u32) -> Node {
    Node {
        id: 0,
        repo_id: "repo".into(),
        path: path.into(),
        symbol: symbol.into(),
        kind: "function".into(),
        line_start: line,
        line_end: line + 3,
        signature: format!("fn {symbol}()"),
    }
}

fn edge(src: NodeId, tgt: NodeId) -> Edge {
    Edge {
        id: 0,
        source_id: src,
        target_id: tgt,
        kind: "CALLS_EXACT".into(),
        weight: 1.0,
    }
}

fn assert_no_dangling_edges(s: &SqliteStorage) {
    for e in s.all_edges().unwrap() {
        assert!(
            s.get_node(e.source_id).unwrap().is_some(),
            "dangling edge: source_id {} references a missing node",
            e.source_id
        );
        assert!(
            s.get_node(e.target_id).unwrap().is_some(),
            "dangling edge: target_id {} references a missing node",
            e.target_id
        );
    }
}

// ─── Required regression test (plan.md §1.2a) ────────────────────────────────

#[test]
fn reindex_one_of_two_mutually_referencing_files_leaves_zero_dangling_edges() {
    let (_dir, path) = tmp_db();
    let mut s = SqliteStorage::open(&path).unwrap();

    let fn_a = s.upsert_node(&node("file_a.rs", "fn_a", 1)).unwrap();
    let fn_b = s.upsert_node(&node("file_b.rs", "fn_b", 1)).unwrap();
    // Mutual reference: file_a.rs calls fn_b; file_b.rs calls fn_a.
    s.upsert_edge(&edge(fn_a, fn_b)).unwrap();
    s.upsert_edge(&edge(fn_b, fn_a)).unwrap();

    // Reindex file_a.rs: edges first, then nodes, then re-insert.
    s.purge_file_edges("repo", "file_a.rs").unwrap();
    s.purge_file_nodes("repo", "file_a.rs").unwrap();

    let new_fn_a = s.upsert_node(&node("file_a.rs", "fn_a", 5)).unwrap();
    s.upsert_edge(&edge(new_fn_a, fn_b)).unwrap();
    // Re-derive the inbound edge from file_b.rs (now points at new_fn_a).
    s.upsert_edge(&edge(fn_b, new_fn_a)).unwrap();

    // Core Invariant 3: zero edge endpoints may reference a missing node.
    assert_no_dangling_edges(&s);
}

// ─── Purge correctness ────────────────────────────────────────────────────────

#[test]
fn purge_file_edges_removes_edges_in_both_directions() {
    let (_dir, path) = tmp_db();
    let mut s = SqliteStorage::open(&path).unwrap();

    let a = s.upsert_node(&node("a.rs", "fn_a", 1)).unwrap();
    let b = s.upsert_node(&node("b.rs", "fn_b", 1)).unwrap();
    let c = s.upsert_node(&node("c.rs", "fn_c", 1)).unwrap();
    // a→b (outbound from a.rs), b→a (inbound to a.rs), c→b (unrelated)
    s.upsert_edge(&edge(a, b)).unwrap();
    s.upsert_edge(&edge(b, a)).unwrap();
    s.upsert_edge(&edge(c, b)).unwrap();

    let purged = s.purge_file_edges("repo", "a.rs").unwrap();
    assert_eq!(purged, 2, "both directions of a.rs edges must be purged");
    assert_eq!(s.all_edges().unwrap().len(), 1);
    assert_no_dangling_edges(&s);
}

#[test]
fn unrelated_file_edges_survive_targeted_purge() {
    let (_dir, path) = tmp_db();
    let mut s = SqliteStorage::open(&path).unwrap();

    let a = s.upsert_node(&node("a.rs", "fn_a", 1)).unwrap();
    let b = s.upsert_node(&node("b.rs", "fn_b", 1)).unwrap();
    let c = s.upsert_node(&node("c.rs", "fn_c", 1)).unwrap();
    s.upsert_edge(&edge(a, b)).unwrap();
    s.upsert_edge(&edge(b, c)).unwrap();

    s.purge_file_edges("repo", "a.rs").unwrap();
    s.purge_file_nodes("repo", "a.rs").unwrap();

    let edges = s.all_edges().unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!((edges[0].source_id, edges[0].target_id), (b, c));
    assert_no_dangling_edges(&s);
}

// ─── Bulk bailout ─────────────────────────────────────────────────────────────

#[test]
fn bailout_threshold_triggers_on_large_change_fraction() {
    let cfg = ReindexConfig::default();
    assert!(!should_bail_out(99, 1000, &cfg), "just under threshold");
    assert!(should_bail_out(101, 1000, &cfg), "just over threshold");
}

#[test]
fn bailout_floor_protects_tiny_repos() {
    let cfg = ReindexConfig::default();
    // 10-file repo: 10% = 1, but floor=100 dominates.
    assert!(!should_bail_out(50, 10, &cfg), "50 < floor=100");
    assert!(should_bail_out(101, 10, &cfg), "101 > floor=100");
}
