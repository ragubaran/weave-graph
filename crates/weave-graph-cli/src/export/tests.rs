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

fn edge(source_id: weave_graph_core::NodeId, target_id: weave_graph_core::NodeId) -> Edge {
    Edge {
        id: 0,
        source_id,
        target_id,
        kind: "CALLS_EXACT".into(),
        weight: 1.0,
        extractor: None,
        resolution_kind: None,
    }
}

// a -> b -> c -> d (chain), plus caller(a) -> a
fn chain_storage() -> (SqliteStorage, Vec<weave_graph_core::NodeId>) {
    let mut s = SqliteStorage::open_in_memory().unwrap();
    let caller = s.upsert_node(&node("caller.rs", "caller")).unwrap();
    let a = s.upsert_node(&node("a.rs", "a")).unwrap();
    let b = s.upsert_node(&node("b.rs", "b")).unwrap();
    let c = s.upsert_node(&node("c.rs", "c")).unwrap();
    let d = s.upsert_node(&node("d.rs", "d")).unwrap();
    s.upsert_edge(&edge(caller, a)).unwrap();
    s.upsert_edge(&edge(a, b)).unwrap();
    s.upsert_edge(&edge(b, c)).unwrap();
    s.upsert_edge(&edge(c, d)).unwrap();
    (s, vec![caller, a, b, c, d])
}

#[test]
fn depth_1_includes_direct_neighbors_in_both_directions() {
    let (storage, ids) = chain_storage();
    let n = neighborhood(&storage, "a", 1, None).unwrap();
    let symbols: Vec<&str> = n.nodes.iter().map(|x| x.symbol.as_str()).collect();
    assert!(symbols.contains(&"a"));
    assert!(
        symbols.contains(&"caller"),
        "direct caller must be included"
    );
    assert!(symbols.contains(&"b"), "direct callee must be included");
    assert!(
        !symbols.contains(&"c"),
        "2 hops away must not be included at depth 1"
    );
    let _ = ids;
}

#[test]
fn depth_2_reaches_two_hops_out() {
    let (storage, _) = chain_storage();
    let n = neighborhood(&storage, "a", 2, None).unwrap();
    let symbols: Vec<&str> = n.nodes.iter().map(|x| x.symbol.as_str()).collect();
    assert!(
        symbols.contains(&"c"),
        "2 hops away must be included at depth 2"
    );
    assert!(
        !symbols.contains(&"d"),
        "3 hops away must not be included at depth 2"
    );
}

#[test]
fn edges_are_limited_to_the_selected_node_set() {
    let (storage, _) = chain_storage();
    let n = neighborhood(&storage, "a", 1, None).unwrap();
    for e in &n.edges {
        let ids: Vec<_> = n.nodes.iter().map(|node| node.id).collect();
        assert!(ids.contains(&e.source_id));
        assert!(ids.contains(&e.target_id));
    }
    // b->c is out of range at depth 1, must not leak in.
    assert!(!n.edges.iter().any(|e| {
        n.nodes
            .iter()
            .any(|x| x.id == e.source_id && x.symbol == "b")
            && n.nodes
                .iter()
                .any(|x| x.id == e.target_id && x.symbol == "c")
    }));
}

#[test]
fn unknown_symbol_is_an_error() {
    let (storage, _) = chain_storage();
    assert!(neighborhood(&storage, "does_not_exist", 2, None).is_err());
}

#[test]
fn mask_is_applied_before_the_exported_neighborhood_is_built() {
    let (storage, _) = chain_storage();
    let mask = |node: &Node| {
        let mut masked = node.clone();
        if masked.symbol == "b" {
            masked.symbol = "<hidden>".to_string();
        }
        masked
    };

    let neighborhood = neighborhood(&storage, "a", 1, Some(&mask)).unwrap();
    assert!(
        neighborhood
            .nodes
            .iter()
            .any(|node| node.symbol == "<hidden>")
    );
    assert!(!neighborhood.nodes.iter().any(|node| node.symbol == "b"));
}

#[test]
fn serializes_to_valid_json_with_expected_shape() {
    let (storage, _) = chain_storage();
    let n = neighborhood(&storage, "a", 1, None).unwrap();
    let json = serde_json::to_string(&n).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["root"], "a");
    assert_eq!(value["depth"], 1);
    assert!(value["nodes"].is_array());
    assert!(value["edges"].is_array());
}

#[cfg(feature = "provenance")]
#[test]
fn node_ids_matches_the_exported_node_set() {
    let (storage, _) = chain_storage();
    let n = neighborhood(&storage, "a", 1, None).unwrap();
    let ids = n.node_ids();
    let expected: Vec<_> = n.nodes.iter().map(|node| node.id).collect();
    assert_eq!(ids.len(), expected.len());
    for id in expected {
        assert!(ids.contains(&id));
    }
}

#[test]
fn inbound_exhausts_before_depth_limit() {
    let (storage, _) = chain_storage();
    // caller -> a -> b.
    // At 'a', caller is at depth 1.
    // If we ask for depth 5, it should exhaust after finding 'caller'.
    let n = neighborhood(&storage, "a", 5, None).unwrap();
    let symbols: Vec<&str> = n.nodes.iter().map(|x| x.symbol.as_str()).collect();
    assert!(symbols.contains(&"caller"));
}

#[test]
fn export_returns_error_if_nodes_fail() {
    struct FailNodes;
    impl Storage for FailNodes {
        fn all_nodes(&self) -> Result<Vec<Node>, weave_graph_core::StorageError> {
            Err(weave_graph_core::StorageError::Backend("failed".into()))
        }
        fn all_edges(&self) -> Result<Vec<Edge>, weave_graph_core::StorageError> {
            Ok(vec![])
        }
        fn get_node(&self, _: NodeId) -> Result<Option<Node>, weave_graph_core::StorageError> {
            Ok(None)
        }
        fn get_callers(&self, _: NodeId) -> Result<Vec<Edge>, weave_graph_core::StorageError> {
            Ok(vec![])
        }
        fn query_path(
            &self,
            _: NodeId,
            _: NodeId,
        ) -> Result<Option<Vec<NodeId>>, weave_graph_core::StorageError> {
            Ok(None)
        }
        fn get_edges(&self, _: NodeId) -> Result<Vec<Edge>, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn upsert_node(
            &mut self,
            _: &weave_graph_core::Node,
        ) -> Result<u32, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn upsert_edge(&mut self, _: &Edge) -> Result<u32, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn schema_version(&self) -> Result<u32, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn purge_file_edges(
            &mut self,
            _: &str,
            _: &str,
        ) -> Result<u64, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn purge_file_nodes(
            &mut self,
            _: &str,
            _: &str,
        ) -> Result<u64, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn pin_note(
            &self,
            _: &weave_graph_core::Note,
        ) -> Result<i64, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn all_notes(&self) -> Result<Vec<weave_graph_core::Note>, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn recall_notes(
            &self,
            _: i64,
        ) -> Result<Vec<weave_graph_core::Note>, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn reattach_note(
            &self,
            _: i64,
            _: std::option::Option<u32>,
            _: bool,
        ) -> Result<(), weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn delete_expired_notes(&self, _: i64) -> Result<u64, weave_graph_core::StorageError> {
            unimplemented!()
        }
    }
    assert!(neighborhood(&FailNodes, "a", 1, None).is_err());
}

#[test]
fn export_returns_error_if_edges_fail() {
    struct FailEdges;
    impl Storage for FailEdges {
        fn all_nodes(&self) -> Result<Vec<Node>, weave_graph_core::StorageError> {
            Ok(vec![node("a.rs", "a")])
        }
        fn all_edges(&self) -> Result<Vec<Edge>, weave_graph_core::StorageError> {
            Err(weave_graph_core::StorageError::Backend("failed".into()))
        }
        fn get_node(&self, _: NodeId) -> Result<Option<Node>, weave_graph_core::StorageError> {
            Ok(None)
        }
        fn get_callers(&self, _: NodeId) -> Result<Vec<Edge>, weave_graph_core::StorageError> {
            Ok(vec![])
        }
        fn query_path(
            &self,
            _: NodeId,
            _: NodeId,
        ) -> Result<Option<Vec<NodeId>>, weave_graph_core::StorageError> {
            Ok(None)
        }
        fn get_edges(&self, _: NodeId) -> Result<Vec<Edge>, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn upsert_node(
            &mut self,
            _: &weave_graph_core::Node,
        ) -> Result<u32, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn upsert_edge(&mut self, _: &Edge) -> Result<u32, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn schema_version(&self) -> Result<u32, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn purge_file_edges(
            &mut self,
            _: &str,
            _: &str,
        ) -> Result<u64, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn purge_file_nodes(
            &mut self,
            _: &str,
            _: &str,
        ) -> Result<u64, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn pin_note(
            &self,
            _: &weave_graph_core::Note,
        ) -> Result<i64, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn all_notes(&self) -> Result<Vec<weave_graph_core::Note>, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn recall_notes(
            &self,
            _: i64,
        ) -> Result<Vec<weave_graph_core::Note>, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn reattach_note(
            &self,
            _: i64,
            _: std::option::Option<u32>,
            _: bool,
        ) -> Result<(), weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn delete_expired_notes(&self, _: i64) -> Result<u64, weave_graph_core::StorageError> {
            unimplemented!()
        }
    }
    assert!(neighborhood(&FailEdges, "a", 1, None).is_err());
}

#[test]
fn export_returns_error_if_csr_load_fails() {
    struct FailCsr;
    impl Storage for FailCsr {
        fn all_nodes(&self) -> Result<Vec<Node>, weave_graph_core::StorageError> {
            Ok(vec![node("a.rs", "a")])
        }
        // all_edges returning an error during CSR load! CsrGraph::load calls all_edges.
        // Wait, CsrGraph::load does call all_edges, so FailEdges covers it.
        fn all_edges(&self) -> Result<Vec<Edge>, weave_graph_core::StorageError> {
            Err(weave_graph_core::StorageError::Backend("failed".into()))
        }
        fn get_node(&self, _: NodeId) -> Result<Option<Node>, weave_graph_core::StorageError> {
            Ok(None)
        }
        fn get_callers(&self, _: NodeId) -> Result<Vec<Edge>, weave_graph_core::StorageError> {
            Ok(vec![])
        }
        fn query_path(
            &self,
            _: NodeId,
            _: NodeId,
        ) -> Result<Option<Vec<NodeId>>, weave_graph_core::StorageError> {
            Ok(None)
        }
        fn get_edges(&self, _: NodeId) -> Result<Vec<Edge>, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn upsert_node(
            &mut self,
            _: &weave_graph_core::Node,
        ) -> Result<u32, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn upsert_edge(&mut self, _: &Edge) -> Result<u32, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn schema_version(&self) -> Result<u32, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn purge_file_edges(
            &mut self,
            _: &str,
            _: &str,
        ) -> Result<u64, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn purge_file_nodes(
            &mut self,
            _: &str,
            _: &str,
        ) -> Result<u64, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn pin_note(
            &self,
            _: &weave_graph_core::Note,
        ) -> Result<i64, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn all_notes(&self) -> Result<Vec<weave_graph_core::Note>, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn recall_notes(
            &self,
            _: i64,
        ) -> Result<Vec<weave_graph_core::Note>, weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn reattach_note(
            &self,
            _: i64,
            _: std::option::Option<u32>,
            _: bool,
        ) -> Result<(), weave_graph_core::StorageError> {
            unimplemented!()
        }
        fn delete_expired_notes(&self, _: i64) -> Result<u64, weave_graph_core::StorageError> {
            unimplemented!()
        }
    }
    assert!(neighborhood(&FailCsr, "a", 1, None).is_err());
}
