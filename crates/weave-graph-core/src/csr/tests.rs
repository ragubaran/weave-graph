use super::*;

struct FakeStorage {
    nodes: Vec<crate::model::Node>,
    edges: Vec<crate::model::Edge>,
}

impl Storage for FakeStorage {
    fn get_node(&self, id: NodeId) -> Result<Option<crate::model::Node>, StorageError> {
        Ok(self.nodes.iter().find(|n| n.id == id).cloned())
    }
    fn get_edges(&self, node_id: NodeId) -> Result<Vec<crate::model::Edge>, StorageError> {
        Ok(self
            .edges
            .iter()
            .filter(|e| e.source_id == node_id)
            .cloned()
            .collect())
    }
    fn get_callers(&self, _: NodeId) -> Result<Vec<crate::model::Edge>, StorageError> {
        unimplemented!("not needed for these tests")
    }
    fn upsert_node(&mut self, _: &crate::model::Node) -> Result<NodeId, StorageError> {
        unimplemented!("CsrGraph::load only reads")
    }
    fn upsert_edge(&mut self, _: &crate::model::Edge) -> Result<u32, StorageError> {
        unimplemented!("CsrGraph::load only reads")
    }
    fn query_path(&self, _: NodeId, _: NodeId) -> Result<Option<Vec<NodeId>>, StorageError> {
        unimplemented!("not needed for these tests")
    }
    fn schema_version(&self) -> Result<u32, StorageError> {
        Ok(2)
    }
    fn all_nodes(&self) -> Result<Vec<crate::model::Node>, StorageError> {
        Ok(self.nodes.clone())
    }
    fn all_edges(&self) -> Result<Vec<crate::model::Edge>, StorageError> {
        Ok(self.edges.clone())
    }
    fn purge_file_edges(&mut self, _: &str, _: &str) -> Result<u64, StorageError> {
        unimplemented!("not needed for these tests")
    }
    fn purge_file_nodes(&mut self, _: &str, _: &str) -> Result<u64, StorageError> {
        unimplemented!("not needed for these tests")
    }

    fn pin_note(&self, _: &crate::notes::Note) -> Result<i64, StorageError> {
        unimplemented!("not needed for these tests")
    }

    fn all_notes(&self) -> Result<Vec<crate::notes::Note>, StorageError> {
        Ok(Vec::new())
    }

    fn recall_notes(&self, _: i64) -> Result<Vec<crate::notes::Note>, StorageError> {
        Ok(Vec::new())
    }

    fn reattach_note(&self, _: i64, _: Option<NodeId>, _: bool) -> Result<(), StorageError> {
        unimplemented!("not needed for these tests")
    }

    fn delete_expired_notes(&self, _: i64) -> Result<u64, StorageError> {
        Ok(0)
    }
}

fn node(id: NodeId) -> crate::model::Node {
    crate::model::Node {
        id,
        repo_id: "r".into(),
        path: format!("{id}.rs"),
        symbol: format!("s{id}"),
        kind: "function".into(),
        line_start: 1,
        line_end: 2,
        signature: String::new(),
    }
}

fn edge(source_id: NodeId, target_id: NodeId, weight: f64) -> crate::model::Edge {
    crate::model::Edge {
        id: 0,
        source_id,
        target_id,
        kind: "CALLS_EXACT".into(),
        weight,
    }
}

/// Storage ids `10, 20, 30` have gaps — proves the compaction actually
/// remaps to a dense `0..n` CSR index space rather than assuming ids
/// are already contiguous.
fn gapped_chain() -> FakeStorage {
    FakeStorage {
        nodes: vec![node(10), node(20), node(30)],
        edges: vec![edge(10, 20, 1.0), edge(20, 30, 1.0)],
    }
}

#[test]
fn loads_gapped_storage_ids_into_a_dense_index_space() {
    let graph = CsrGraph::load(&gapped_chain()).unwrap();
    assert_eq!(graph.node_count(), 3);
    assert_eq!(graph.edge_count(), 2);
    assert_eq!(graph.outbound(10), vec![20]);
}

#[test]
fn query_path_matches_the_sql_bfs_semantics() {
    let graph = CsrGraph::load(&gapped_chain()).unwrap();
    assert_eq!(graph.query_path(10, 30), Some(vec![10, 20, 30]));
    assert_eq!(graph.query_path(10, 10), Some(vec![10]));
    assert_eq!(graph.query_path(30, 10), None, "edges are directed");
}

#[test]
fn duplicate_source_target_pairs_with_different_kinds_collapse_to_one_edge() {
    let storage = FakeStorage {
        nodes: vec![node(1), node(2)],
        edges: vec![
            crate::model::Edge {
                id: 0,
                source_id: 1,
                target_id: 2,
                kind: "CALLS_EXACT".into(),
                weight: 1.0,
            },
            crate::model::Edge {
                id: 1,
                source_id: 1,
                target_id: 2,
                kind: "IMPORTS".into(),
                weight: 5.0,
            },
        ],
    };
    let graph = CsrGraph::load(&storage).unwrap();
    assert_eq!(graph.edge_count(), 1);
    assert_eq!(graph.outbound(1), vec![2]);
}

#[test]
fn trailing_disconnected_nodes_are_still_counted() {
    let storage = FakeStorage {
        nodes: vec![node(1), node(2), node(3)],
        edges: vec![],
    };
    let graph = CsrGraph::load(&storage).unwrap();
    assert_eq!(graph.node_count(), 3);
    assert!(graph.outbound(3).is_empty());
}

#[test]
fn reachable_within_bounds_by_hop_count_and_supports_set_intersection() {
    // 1 -> 2 -> 3 -> 4
    let storage = FakeStorage {
        nodes: vec![node(1), node(2), node(3), node(4)],
        edges: vec![edge(1, 2, 1.0), edge(2, 3, 1.0), edge(3, 4, 1.0)],
    };
    let graph = CsrGraph::load(&storage).unwrap();

    let within_one = graph.reachable_within(1, 1);
    assert_eq!(within_one.len(), 2, "the start node plus its one hop");

    let within_two = graph.reachable_within(1, 2);
    assert_eq!(within_two.len(), 3);

    let from_far_end = graph.reachable_within(4, 5);
    let overlap = &within_two & &from_far_end;
    assert!(
        overlap.is_empty(),
        "1's 2-hop radius and 4's radius share no node in this chain"
    );
}

/// `callers_within` must find real multi-hop callers by walking edges
/// *backward* — a capability `reachable_within` (outbound-only) never had,
/// and `Storage::get_callers`'s SQL path never bounded by depth.
#[test]
fn callers_within_bounds_by_hop_count_over_the_reverse_direction() {
    // 1 -> 2 -> 3 -> 4 (1 calls 2, 2 calls 3, 3 calls 4)
    let storage = FakeStorage {
        nodes: vec![node(1), node(2), node(3), node(4)],
        edges: vec![edge(1, 2, 1.0), edge(2, 3, 1.0), edge(3, 4, 1.0)],
    };
    let graph = CsrGraph::load(&storage).unwrap();

    // Who calls 4, within 1 hop? Just 3 (plus 4 itself).
    let within_one = graph.callers_within(4, 1);
    assert_eq!(within_one.len(), 2);
    assert!(within_one.contains(graph_index(&graph, 3)));
    assert!(!within_one.contains(graph_index(&graph, 2)));

    // Within 2 hops: 3 and 2 (transitively, 2 calls 3 calls 4).
    let within_two = graph.callers_within(4, 2);
    assert_eq!(within_two.len(), 3);
    assert!(within_two.contains(graph_index(&graph, 2)));
    assert!(!within_two.contains(graph_index(&graph, 1)));

    // Forward traversal from the same node must not find its own callers.
    assert!(
        graph.reachable_within(4, 3).len() == 1,
        "4 calls nothing downstream in this chain"
    );

    // Directed both ways: node 1 (the root caller) has no callers of its own.
    assert_eq!(graph.callers_within(1, 5).len(), 1);
}

fn graph_index(graph: &CsrGraph, id: NodeId) -> u32 {
    graph.index_to_id.binary_search(&id).unwrap() as u32
}

#[test]
fn fake_storage_get_node_get_edges_and_schema_version_behave_sanely() {
    let storage = gapped_chain();
    assert_eq!(storage.get_node(20).unwrap().unwrap().symbol, "s20");
    assert!(storage.get_node(999).unwrap().is_none());
    assert_eq!(storage.get_edges(10).unwrap().len(), 1);
    assert_eq!(storage.schema_version().unwrap(), 2);
}

#[test]
#[should_panic(expected = "CsrGraph::load only reads")]
fn fake_storage_upsert_node_is_a_write_stub_this_test_double_never_needs() {
    let _ = gapped_chain().upsert_node(&node(1));
}

#[test]
#[should_panic(expected = "CsrGraph::load only reads")]
fn fake_storage_upsert_edge_is_a_write_stub_this_test_double_never_needs() {
    let _ = gapped_chain().upsert_edge(&edge(1, 2, 1.0));
}

#[test]
#[should_panic(expected = "not needed for these tests")]
fn fake_storage_query_path_is_a_stub_csr_graph_never_calls() {
    let _ = gapped_chain().query_path(1, 2);
}

#[test]
fn from_nodes_and_edges_reconstructs_identical_csr_structure() {
    let nodes = vec![10, 20, 30];
    let edges = vec![(10, 20, 1.0), (20, 30, 2.0), (10, 999, 1.0)];
    let graph = CsrGraph::from_nodes_and_edges(&nodes, &edges).unwrap();
    assert_eq!(graph.node_count(), 3);
    assert_eq!(graph.edge_count(), 2);
    assert_eq!(graph.outbound(10), vec![20]);
    assert_eq!(graph.query_path(10, 30), Some(vec![10, 20, 30]));
}

#[test]
fn from_nodes_and_edges_dedups_duplicate_pairs_regardless_of_the_weight_argument() {
    let nodes = vec![1, 2];
    // The weight component is accepted for caller convenience (WASM
    // callers often already have it) but never stored — see `CsrGraph`'s
    // own doc comment — so two duplicate pairs with different weights
    // still collapse to one edge.
    let edges = vec![(1, 2, 1.0), (1, 2, 5.0)];
    let graph = CsrGraph::from_nodes_and_edges(&nodes, &edges).unwrap();
    assert_eq!(graph.edge_count(), 1);
    assert_eq!(graph.outbound(1), vec![2]);
}

/// `reverse_csr` must not be built at load time — every consumer that
/// never calls `callers_within` (`weave query`/`report`/`export`, every MCP
/// tool, `weave blast --direction callees`) must never pay to build or
/// hold it.
#[test]
fn reverse_csr_is_not_built_until_callers_within_is_first_called() {
    let graph = CsrGraph::load(&gapped_chain()).unwrap();
    assert!(
        graph.reverse_csr.get().is_none(),
        "must not build the reverse CSR eagerly at load time"
    );

    let _ = graph.callers_within(30, 1);

    assert!(
        graph.reverse_csr.get().is_some(),
        "must build it lazily on first callers_within call"
    );
}

#[test]
fn empty_and_default_graphs_have_zero_size() {
    let empty = CsrGraph::empty();
    assert_eq!(empty.node_count(), 0);
    assert_eq!(empty.edge_count(), 0);
    assert!(empty.outbound(1).is_empty());
    assert_eq!(empty.id_of_index(0), None);
    assert!(empty.reachable_nodes(1, 1).is_empty());

    let def = CsrGraph::default();
    assert_eq!(def.node_count(), 0);
}

#[test]
fn id_of_index_and_reachable_nodes_map_correctly() {
    let nodes = vec![100, 200, 300];
    let edges = vec![(100, 200, 1.0), (200, 300, 1.0)];
    let graph = CsrGraph::from_nodes_and_edges(&nodes, &edges).unwrap();

    assert_eq!(graph.id_of_index(0), Some(100));
    assert_eq!(graph.id_of_index(1), Some(200));
    assert_eq!(graph.id_of_index(999), None);

    let reachable = graph.reachable_nodes(100, 1);
    assert_eq!(reachable.len(), 2);
    assert!(reachable.contains(&100));
    assert!(reachable.contains(&200));
    assert!(!reachable.contains(&300));
}
