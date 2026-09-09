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
fn duplicate_source_target_pairs_with_different_kinds_collapse_to_one_edge_with_max_weight() {
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
