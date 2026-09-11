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
    let result = weave_impact_radius(
        &storage,
        &csr,
        ImpactRadiusArgs {
            symbol: "root",
            max_tokens: None,
        },
        None,
    );
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
    let result = weave_impact_radius(
        &storage,
        &csr,
        ImpactRadiusArgs {
            symbol: "a",
            max_tokens: None,
        },
        None,
    );
    assert_eq!(result.symbol_count, 1, "only b is impacted, no duplicate a");
}

#[test]
fn unknown_symbol_returns_not_found() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let csr = CsrGraph::load(&storage).unwrap();
    let result = weave_impact_radius(
        &storage,
        &csr,
        ImpactRadiusArgs {
            symbol: "ghost",
            max_tokens: None,
        },
        None,
    );
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
    let result = weave_impact_radius(
        &storage,
        &csr,
        ImpactRadiusArgs {
            symbol: "s1",
            max_tokens: None,
        },
        None,
    );
    assert_eq!(result.symbol_count, 21);
    assert!(result.text.contains("... and 1 more"));
}

// ─── impl.md M2.16: token-budgeted shedding ─────────────────────────────────

/// A synthetic hub: `root` fans out to 100 downstream symbols.
fn hub_storage() -> (SqliteStorage, CsrGraph) {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
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
    for i in 0..100 {
        let leaf = storage
            .upsert_node(&Node {
                id: 10 + i,
                repo_id: "r".into(),
                path: format!("leafmod{}.rs", i / 10),
                symbol: format!("leaf{i}"),
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
    (storage, csr)
}

#[test]
fn small_max_tokens_sheds_a_hub_to_a_summary_not_an_unbounded_list() {
    let (storage, csr) = hub_storage();
    let result = weave_impact_radius(
        &storage,
        &csr,
        ImpactRadiusArgs {
            symbol: "root",
            max_tokens: Some(40),
        },
        None,
    );
    assert!(
        result.text.contains("shed to"),
        "tier marker present: {}",
        result.text
    );
    assert!(
        result.text.contains("100 symbols affected"),
        "{}",
        result.text
    );
    // The shed output must actually be within the estimate.
    assert!(
        crate::tools::estimate_tokens(&result.text) <= 40,
        "{}",
        result.text
    );
    // The full per-symbol list (first 20 leaves) must not be present.
    assert!(
        !result.text.contains("leaf0 (leaf0.rs:1)"),
        "{}",
        result.text
    );
}

#[test]
fn omitted_max_tokens_returns_the_legacy_full_format() {
    let (storage, csr) = hub_storage();
    let result = weave_impact_radius(
        &storage,
        &csr,
        ImpactRadiusArgs {
            symbol: "root",
            max_tokens: None,
        },
        None,
    );
    // Byte-identical to the M1.7-era format: 20 symbols + "and N more".
    assert!(
        result
            .text
            .contains("impact_radius: root (100 symbols affected)")
    );
    assert!(result.text.contains("leaf0 (leafmod0.rs:1)"));
    assert!(result.text.contains("leaf19 (leafmod1.rs:1)"));
    assert!(
        !result.text.contains("leaf20 ("),
        "legacy take(20) cap intact"
    );
    assert!(result.text.contains("... and 80 more"));
}
