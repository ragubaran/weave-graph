#![deny(unsafe_code)]

// 1. Standard library

// 2. Third-party crates
use wasm_bindgen::prelude::*;

// 3. First-party workspace crates
use weave_graph_core::{CsrGraph, NodeId, louvain_communities};

// WebAssembly-accessible wrapper over the integer-compacted CSR graph.
// Preserves zero-network, in-memory execution inside browsers and VS Code Web.
#[wasm_bindgen]
pub struct WasmGraph {
    csr: CsrGraph,
    nodes: Vec<NodeId>,
    edges: Vec<(NodeId, NodeId, f64)>,
}

#[wasm_bindgen]
impl WasmGraph {
    // Zero-argument constructor creating an empty graph in WASM memory.
    // Enables client-side instantiation prior to asynchronous snapshot loading.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            csr: CsrGraph::empty(),
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    // Hydrates graph from a list of node IDs and flattened edge triplets.
    // Preserves compact array passing without JSON stringify overhead.
    #[wasm_bindgen]
    pub fn load_flat(nodes: &[u32], edges_flat: &[f64]) -> Result<WasmGraph, String> {
        if !edges_flat.len().is_multiple_of(3) {
            return Err(
                "edges_flat length must be an exact multiple of 3 (src, target, weight)"
                    .to_string(),
            );
        }
        let edge_count = edges_flat.len() / 3;
        let mut edges = Vec::with_capacity(edge_count);
        for i in 0..edge_count {
            let src = edges_flat[i * 3] as u32;
            let tgt = edges_flat[i * 3 + 1] as u32;
            let weight = edges_flat[i * 3 + 2];
            edges.push((src, tgt, weight));
        }
        let csr = CsrGraph::from_nodes_and_edges(nodes, &edges).map_err(|e| e.to_string())?;
        Ok(Self {
            csr,
            nodes: nodes.to_vec(),
            edges,
        })
    }

    // Hydrates graph from JSON string representations of nodes and edges.
    // Enables direct consumption of exported .canvas and report graph slices.
    #[wasm_bindgen]
    pub fn load_json(nodes_json: &str, edges_json: &str) -> Result<WasmGraph, String> {
        let nodes: Vec<u32> =
            serde_json::from_str(nodes_json).map_err(|e| format!("Invalid nodes JSON: {e}"))?;
        let raw_edges: Vec<(u32, u32, f64)> =
            serde_json::from_str(edges_json).map_err(|e| format!("Invalid edges JSON: {e}"))?;
        let csr = CsrGraph::from_nodes_and_edges(&nodes, &raw_edges).map_err(|e| e.to_string())?;
        Ok(Self {
            csr,
            nodes,
            edges: raw_edges,
        })
    }

    // Number of distinct nodes compacted into the CSR matrix.
    #[wasm_bindgen]
    pub fn node_count(&self) -> usize {
        self.csr.node_count()
    }

    // Number of compacted, deduplicated edges in the CSR matrix.
    #[wasm_bindgen]
    pub fn edge_count(&self) -> usize {
        self.csr.edge_count()
    }

    // Outbound direct neighbor IDs in CSR storage order.
    #[wasm_bindgen]
    pub fn outbound(&self, id: u32) -> Vec<u32> {
        self.csr.outbound(id)
    }

    // Unweighted shortest path using RoaringBitmap BFS traversal.
    #[wasm_bindgen]
    pub fn query_path(&self, from: u32, to: u32) -> Option<Vec<u32>> {
        self.csr.query_path(from, to)
    }

    // Transitive reachability within max_hops steps from start node.
    // Backed by compact RoaringBitmap set operations in WebAssembly memory.
    #[wasm_bindgen]
    pub fn reachable_within(&self, from: u32, max_hops: u32) -> Vec<u32> {
        self.csr.reachable_nodes(from, max_hops)
    }

    // Partitions graph into architectural clusters via Louvain modularity.
    // Serializes community map to JSON to avoid complex JS FFI type marshalling.
    #[wasm_bindgen]
    pub fn cluster_louvain(&self) -> Result<String, String> {
        let communities = louvain_communities(&self.nodes, &self.edges);
        serde_json::to_string(&communities)
            .map_err(|e| format!("Failed to serialize communities: {e}"))
    }
}

impl Default for WasmGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
