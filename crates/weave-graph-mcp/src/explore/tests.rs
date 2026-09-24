use super::*;
use crate::tools::ExploreArgs;
use weave_graph_core::{Edge, Node};
use weave_graph_store_sqlite::SqliteStorage;

fn node(symbol: &str, path: &str) -> Node {
    Node {
        id: 0,
        repo_id: "r".into(),
        path: path.into(),
        symbol: symbol.into(),
        kind: "function".into(),
        line_start: 2,
        line_end: 3,
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
fn no_symbol_falls_back_to_a_repo_overview() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let csr = CsrGraph::load(&storage).unwrap();
    let result = weave_explore(
        &storage,
        &csr,
        None,
        ExploreArgs {
            symbol: None,
            max_tokens: None,
        },
        None,
    );
    assert!(result.text.contains("repo overview"), "{}", result.text);
    assert!(result.text.contains("resident_tokens"), "{}", result.text);
}

#[test]
fn unknown_symbol_returns_not_found() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let csr = CsrGraph::load(&storage).unwrap();
    let result = weave_explore(
        &storage,
        &csr,
        None,
        ExploreArgs {
            symbol: Some("nope"),
            max_tokens: None,
        },
        None,
    );
    assert!(result.text.contains("symbol not found"), "{}", result.text);
}

#[test]
fn composes_file_api_trace_and_impact_for_a_known_symbol() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("root", "root.rs")).unwrap();
    let b = storage.upsert_node(&node("callee", "callee.rs")).unwrap();
    storage.upsert_edge(&edge(a, b)).unwrap();
    let csr = CsrGraph::load(&storage).unwrap();

    let result = weave_explore(
        &storage,
        &csr,
        None,
        ExploreArgs {
            symbol: Some("root"),
            max_tokens: None,
        },
        None,
    );
    assert!(result.text.contains("file API"), "{}", result.text);
    assert!(result.text.contains("call trace"), "{}", result.text);
    assert!(result.text.contains("impact radius"), "{}", result.text);
    assert!(result.text.contains("root.rs"), "{}", result.text);
    assert!(
        result
            .text
            .contains("no repo root, redacted path, or file not found"),
        "no repo_root was given, so the excerpt must say so: {}",
        result.text
    );
}

#[test]
fn reads_a_real_source_excerpt_when_a_repo_root_is_given() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("root.rs"),
        "// line 1\nfn root() {\n    callee();\n}\n",
    )
    .unwrap();
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage.upsert_node(&node("root", "root.rs")).unwrap();
    let csr = CsrGraph::load(&storage).unwrap();

    let result = weave_explore(
        &storage,
        &csr,
        Some(dir.path()),
        ExploreArgs {
            symbol: Some("root"),
            max_tokens: None,
        },
        None,
    );
    assert!(
        result.text.contains("fn root() {") && result.text.contains("callee();"),
        "{}",
        result.text
    );
}

#[test]
fn a_small_budget_sheds_the_excerpt_before_the_call_trace_and_impact_radius() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("root.rs"),
        "// line 1\nfn root() {\n    callee();\n}\n",
    )
    .unwrap();
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("root", "root.rs")).unwrap();
    let b = storage.upsert_node(&node("callee", "callee.rs")).unwrap();
    storage.upsert_edge(&edge(a, b)).unwrap();
    let csr = CsrGraph::load(&storage).unwrap();

    let result = weave_explore(
        &storage,
        &csr,
        Some(dir.path()),
        ExploreArgs {
            symbol: Some("root"),
            max_tokens: Some(1),
        },
        None,
    );
    assert!(result.text.contains("file API"), "{}", result.text);
    assert!(
        !result.text.contains("fn root() {"),
        "the excerpt must be the first thing shed: {}",
        result.text
    );
    assert!(result.text.contains("resident_tokens"), "{}", result.text);
    assert!(result.text.contains("budget: 1"), "{}", result.text);
}

#[test]
fn mask_is_applied_before_resolution_like_impact_radius() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let a = storage.upsert_node(&node("root", "root.rs")).unwrap();
    let b = storage.upsert_node(&node("callee", "callee.rs")).unwrap();
    storage.upsert_edge(&edge(a, b)).unwrap();
    let csr = CsrGraph::load(&storage).unwrap();

    let mask: &dyn Fn(&Node) -> Node = &|n: &Node| {
        if n.symbol == "root" {
            n.clone()
        } else {
            Node {
                symbol: format!("masked-{}", n.symbol),
                ..n.clone()
            }
        }
    };
    let result = weave_explore(
        &storage,
        &csr,
        None,
        ExploreArgs {
            symbol: Some("root"),
            max_tokens: None,
        },
        Some(mask),
    );
    assert!(result.text.contains("masked-callee"), "{}", result.text);
}
