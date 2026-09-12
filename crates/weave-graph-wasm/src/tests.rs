use std::collections::HashMap;

use super::*;

#[test]
fn new_and_default_instantiate_empty_graph() {
    let graph = WasmGraph::new();
    assert_eq!(graph.node_count(), 0);
    assert_eq!(graph.edge_count(), 0);
    assert!(graph.outbound(1).is_empty());
    assert_eq!(graph.query_path(1, 2), None);
    assert!(graph.reachable_within(1, 2).is_empty());

    let def_graph = WasmGraph::default();
    assert_eq!(def_graph.node_count(), 0);
    assert_eq!(def_graph.edge_count(), 0);
}

#[test]
fn load_flat_indexes_dense_and_gapped_nodes() {
    let nodes = [10, 20, 30];
    let edges_flat = [10.0, 20.0, 1.0, 20.0, 30.0, 2.0];
    let graph = WasmGraph::load_flat(&nodes, &edges_flat).expect("load_flat should succeed");

    assert_eq!(graph.node_count(), 3);
    assert_eq!(graph.edge_count(), 2);
    assert_eq!(graph.outbound(10), vec![20]);
    assert_eq!(graph.query_path(10, 30), Some(vec![10, 20, 30]));
    assert_eq!(graph.query_path(30, 10), None);

    let reachable = graph.reachable_within(10, 1);
    assert_eq!(reachable.len(), 2);
    assert!(reachable.contains(&10));
    assert!(reachable.contains(&20));
}

#[test]
fn load_flat_rejects_non_triplet_slice_lengths() {
    let nodes = [1, 2];
    let invalid_edges = [1.0, 2.0];
    let res = WasmGraph::load_flat(&nodes, &invalid_edges);
    let err = match res {
        Err(e) => e,
        Ok(_) => panic!("non-multiple of 3 must fail"),
    };
    assert!(err.contains("multiple of 3"));
}

#[test]
fn load_json_hydrates_valid_structures() {
    let nodes_json = "[1, 2, 3, 4]";
    let edges_json = "[[1, 2, 1.0], [2, 3, 1.5], [3, 4, 2.0]]";
    let graph = match WasmGraph::load_json(nodes_json, edges_json) {
        Ok(g) => g,
        Err(e) => panic!("load_json failed: {e}"),
    };

    assert_eq!(graph.node_count(), 4);
    assert_eq!(graph.edge_count(), 3);
    assert_eq!(graph.query_path(1, 4), Some(vec![1, 2, 3, 4]));
}

#[test]
fn load_json_reports_parse_errors_on_malformed_inputs() {
    let bad_nodes = "{not an array}";
    let err_nodes = match WasmGraph::load_json(bad_nodes, "[]") {
        Err(e) => e,
        Ok(_) => panic!("bad nodes JSON must fail"),
    };
    assert!(err_nodes.contains("Invalid nodes JSON"));

    let bad_edges = "[[1, 2]]";
    let err_edges = match WasmGraph::load_json("[1, 2]", bad_edges) {
        Err(e) => e,
        Ok(_) => panic!("bad edges JSON must fail"),
    };
    assert!(err_edges.contains("Invalid edges JSON"));
}

#[test]
fn cluster_louvain_serializes_valid_partition_map() {
    let nodes = [1, 2, 3, 4];
    let edges_flat = [1.0, 2.0, 1.0, 2.0, 1.0, 1.0, 3.0, 4.0, 1.0];
    let graph = WasmGraph::load_flat(&nodes, &edges_flat).expect("load_flat should succeed");

    let json_str = graph.cluster_louvain().expect("clustering must succeed");
    let map: HashMap<u32, u32> = serde_json::from_str(&json_str).expect("must parse community map");
    assert_eq!(map.len(), 4);

    let empty_graph = WasmGraph::new();
    let empty_json = empty_graph
        .cluster_louvain()
        .expect("empty cluster must succeed");
    let empty_map: HashMap<u32, u32> = serde_json::from_str(&empty_json).expect("must parse empty");
    assert!(empty_map.is_empty());
}

#[test]
fn query_on_missing_nodes_returns_safe_fallbacks() {
    let nodes = [1, 2];
    let edges_flat = [1.0, 2.0, 1.0];
    let graph = WasmGraph::load_flat(&nodes, &edges_flat).expect("load_flat should succeed");

    assert!(graph.outbound(999).is_empty());
    assert_eq!(graph.query_path(999, 2), None);
    assert_eq!(graph.query_path(1, 999), None);
    assert!(graph.reachable_within(999, 3).is_empty());
}
