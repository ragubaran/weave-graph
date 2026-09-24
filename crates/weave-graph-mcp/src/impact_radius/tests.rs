use super::*;
use crate::tools::ImpactRadiusArgs;
use weave_graph_core::{Edge, Node, Storage, StorageError};
use weave_graph_store_sqlite::SqliteStorage;

/// A `Storage` whose `all_nodes` always errors — `weave_impact_radius`'s
/// only escape hatch for a real backend failure.
struct FailingStorage;

impl Storage for FailingStorage {
    fn get_node(&self, _: weave_graph_core::NodeId) -> Result<Option<Node>, StorageError> {
        unimplemented!("not needed for this test")
    }
    fn get_edges(&self, _: weave_graph_core::NodeId) -> Result<Vec<Edge>, StorageError> {
        unimplemented!("not needed for this test")
    }
    fn get_callers(&self, _: weave_graph_core::NodeId) -> Result<Vec<Edge>, StorageError> {
        unimplemented!("not needed for this test")
    }
    fn upsert_node(&mut self, _: &Node) -> Result<weave_graph_core::NodeId, StorageError> {
        unimplemented!("not needed for this test")
    }
    fn upsert_edge(&mut self, _: &Edge) -> Result<u32, StorageError> {
        unimplemented!("not needed for this test")
    }
    fn query_path(
        &self,
        _: weave_graph_core::NodeId,
        _: weave_graph_core::NodeId,
    ) -> Result<Option<Vec<weave_graph_core::NodeId>>, StorageError> {
        unimplemented!("not needed for this test")
    }
    fn schema_version(&self) -> Result<u32, StorageError> {
        unimplemented!("not needed for this test")
    }
    fn all_nodes(&self) -> Result<Vec<Node>, StorageError> {
        Err(StorageError::Backend("disk on fire".to_string()))
    }
    fn all_edges(&self) -> Result<Vec<Edge>, StorageError> {
        unimplemented!("not needed for this test")
    }
    fn purge_file_edges(&mut self, _: &str, _: &str) -> Result<u64, StorageError> {
        unimplemented!("not needed for this test")
    }
    fn purge_file_nodes(&mut self, _: &str, _: &str) -> Result<u64, StorageError> {
        unimplemented!("not needed for this test")
    }
    fn pin_note(&self, _: &weave_graph_core::Note) -> Result<i64, StorageError> {
        unimplemented!("not needed for this test")
    }
    fn all_notes(&self) -> Result<Vec<weave_graph_core::Note>, StorageError> {
        unimplemented!("not needed for this test")
    }
    fn recall_notes(&self, _: i64) -> Result<Vec<weave_graph_core::Note>, StorageError> {
        unimplemented!("not needed for this test")
    }
    fn reattach_note(
        &self,
        _: i64,
        _: Option<weave_graph_core::NodeId>,
        _: bool,
    ) -> Result<(), StorageError> {
        unimplemented!("not needed for this test")
    }
    fn delete_expired_notes(&self, _: i64) -> Result<u64, StorageError> {
        unimplemented!("not needed for this test")
    }
}

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
fn storage_error_is_reported_not_panicked() {
    let empty = SqliteStorage::open_in_memory().unwrap();
    let csr = CsrGraph::load(&empty).unwrap();
    let result = weave_impact_radius(
        &FailingStorage,
        &csr,
        ImpactRadiusArgs {
            symbol: "root",
            max_tokens: None,
        },
        None,
    );
    assert_eq!(result.symbol_count, 0);
    assert!(result.text.contains("disk on fire"), "{}", result.text);
}

#[test]
fn mask_is_applied_to_every_node_before_rendering() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let id1 = storage.upsert_node(&node("root")).unwrap();
    let id2 = storage.upsert_node(&node("b")).unwrap();
    storage.upsert_edge(&edge(id1, id2)).unwrap();
    let csr = CsrGraph::load(&storage).unwrap();

    // Leaves "root" itself alone — masking the requested root symbol too
    // would make it unresolvable by name, which isn't what this test is
    // checking (that's a real, separate RBAC consideration, not this bug).
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
    let result = weave_impact_radius(
        &storage,
        &csr,
        ImpactRadiusArgs {
            symbol: "root",
            max_tokens: None,
        },
        Some(mask),
    );
    // "root" itself is resolved via the masked list too, so masking it
    // must not break symbol resolution — only "b" (the impacted node)
    // shows up in the rendered text.
    assert!(result.text.contains("masked-b"), "{}", result.text);
}

#[test]
fn very_small_max_tokens_sheds_a_hub_all_the_way_to_module_summary() {
    let (storage, csr) = hub_storage();
    let result = weave_impact_radius(
        &storage,
        &csr,
        ImpactRadiusArgs {
            symbol: "root",
            max_tokens: Some(3),
        },
        None,
    );
    assert!(
        result.text.contains("shed to module summary"),
        "{}",
        result.text
    );
    assert_eq!(result.symbol_count, 100);
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

// ─── token-budgeted shedding ─────────────────────────────────

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
                extractor: None,
                resolution_kind: None,
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
    // Byte-identical to the un-truncated format: 20 symbols + "and N more".
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
