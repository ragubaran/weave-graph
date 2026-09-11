//! `impl.md` M1.3's required bench: bytes-per-node ≤24, bytes-per-edge ≤8
//! are **targets, not gates yet** (see M1.9). This measures load time via
//! criterion and reports byte-size analytically rather than via runtime
//! heap introspection — `petgraph::csr::Csr`'s internal `Vec`s aren't
//! exposed for that, and adding a heap-profiling dependency just for this
//! diagnostic isn't worth it at this milestone.
//!
//! Layout (`Csr<(), f64, Directed, u32>`): `row: Vec<u32>` (len n+1),
//! `column: Vec<u32>` (len m), `edges: Vec<f64>` (len m), no node weight
//! storage (`N = ()`). So bytes/node ≈ 4 (row entry), bytes/edge ≈ 12
//! (4-byte column index + 8-byte weight) — already over the 8-byte/edge
//! target with an `f64` weight; narrowing that is real M1.9 work, not
//! something to fudge here. `CsrGraph`'s own id-compaction maps
//! (`HashMap<NodeId, u32>` + `Vec<NodeId>`) add further overhead on top,
//! reported separately since they're this crate's wrapper, not the CSR
//! itself.

use criterion::{Criterion, criterion_group, criterion_main};
use weave_graph_core::{CsrGraph, Edge, Node, NodeId, Storage, StorageError};

struct ChainStorage {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

impl ChainStorage {
    /// `n` nodes in a straight chain `0 -> 1 -> 2 -> ... -> n-1`.
    fn chain(n: u32) -> Self {
        let nodes = (0..n)
            .map(|i| Node {
                id: i,
                repo_id: "r".into(),
                path: format!("f{i}.rs"),
                symbol: format!("s{i}"),
                kind: "function".into(),
                line_start: 1,
                line_end: 2,
                signature: String::new(),
            })
            .collect();
        let edges = (0..n.saturating_sub(1))
            .map(|i| Edge {
                id: i,
                source_id: i,
                target_id: i + 1,
                kind: "CALLS_EXACT".into(),
                weight: 1.0,
            })
            .collect();
        Self { nodes, edges }
    }
}

impl Storage for ChainStorage {
    fn get_node(&self, id: NodeId) -> Result<Option<Node>, StorageError> {
        Ok(self.nodes.get(id as usize).cloned())
    }
    fn get_edges(&self, node_id: NodeId) -> Result<Vec<Edge>, StorageError> {
        Ok(self
            .edges
            .iter()
            .filter(|e| e.source_id == node_id)
            .cloned()
            .collect())
    }
    fn get_callers(&self, _: NodeId) -> Result<Vec<Edge>, StorageError> {
        unimplemented!("bench only reads")
    }
    fn upsert_node(&mut self, _: &Node) -> Result<NodeId, StorageError> {
        unimplemented!("bench only reads")
    }
    fn upsert_edge(&mut self, _: &Edge) -> Result<u32, StorageError> {
        unimplemented!("bench only reads")
    }
    fn query_path(&self, _: NodeId, _: NodeId) -> Result<Option<Vec<NodeId>>, StorageError> {
        unimplemented!("not needed for this bench")
    }
    fn schema_version(&self) -> Result<u32, StorageError> {
        Ok(2)
    }
    fn all_nodes(&self) -> Result<Vec<Node>, StorageError> {
        Ok(self.nodes.clone())
    }
    fn all_edges(&self) -> Result<Vec<Edge>, StorageError> {
        Ok(self.edges.clone())
    }
    fn purge_file_edges(&mut self, _: &str, _: &str) -> Result<u64, StorageError> {
        unimplemented!("bench only reads")
    }
    fn purge_file_nodes(&mut self, _: &str, _: &str) -> Result<u64, StorageError> {
        unimplemented!("bench only reads")
    }

    fn pin_note(&self, _: &weave_graph_core::notes::Note) -> Result<i64, StorageError> {
        unimplemented!("bench only reads")
    }

    fn all_notes(&self) -> Result<Vec<weave_graph_core::notes::Note>, StorageError> {
        Ok(Vec::new())
    }

    fn recall_notes(&self, _: i64) -> Result<Vec<weave_graph_core::notes::Note>, StorageError> {
        Ok(Vec::new())
    }

    fn reattach_note(&self, _: i64, _: Option<NodeId>, _: bool) -> Result<(), StorageError> {
        unimplemented!("bench only reads")
    }

    fn delete_expired_notes(&self, _: i64) -> Result<u64, StorageError> {
        Ok(0)
    }
}

fn csr_memory(c: &mut Criterion) {
    let mut group = c.benchmark_group("csr_load");

    for size in [1_000u32, 50_000, 200_000] {
        let storage = ChainStorage::chain(size);
        let node_count = storage.nodes.len();
        let edge_count = storage.edges.len();

        // Analytical estimate — see module doc for the layout this assumes.
        let csr_core_bytes = (node_count + 1) * 4 + edge_count * (4 + 8);
        let compaction_bytes = node_count * (4 + 4); // id_to_index entry + index_to_id entry, ignoring HashMap load-factor overhead
        eprintln!(
            "n={node_count} m={edge_count}: csr core ~{:.1} bytes/node, ~{:.1} bytes/edge; +compaction maps ~{:.1} bytes/node",
            csr_core_bytes as f64 / node_count as f64,
            (edge_count * 12) as f64 / edge_count.max(1) as f64,
            compaction_bytes as f64 / node_count as f64,
        );

        group.bench_with_input(
            criterion::BenchmarkId::new("load", size),
            &storage,
            |b, storage| {
                b.iter(|| CsrGraph::load(storage).unwrap());
            },
        );
    }

    group.finish();
}

criterion_group!(benches, csr_memory);
criterion_main!(benches);
