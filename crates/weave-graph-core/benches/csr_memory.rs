//! Bytes-per-node/edge targets for the resource envelope: ≤24 bytes/node,
//! ≤8 bytes/edge. Measures load time via criterion and reports byte-size
//! analytically for `CompactCsr` (`row_offsets: Vec<usize>`,
//! `column_indices: Vec<u32>`) plus `CsrGraph`'s `index_to_id: Vec<NodeId>`.
//!
//! Layout (`CompactCsr` in `weave-graph-core/src/csr.rs`): `row_offsets:
//! Vec<usize>` (len n+1, 8 bytes/entry on 64-bit), `column_indices:
//! Vec<u32>` (len m, 4 bytes/entry), and `index_to_id: Vec<NodeId>` (len n,
//! 4 bytes/entry). Forward load costs 12 bytes/node + 4 bytes/edge;
//! `callers_within` builds a temporary reverse `CompactCsr` (+8 bytes/node,
//! +4 bytes/edge during traversal only) and drops it on return.

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
                extractor: None,
                resolution_kind: None,
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

        // CompactCsr stores u32 row offsets and u32 column indices.
        // Compaction uses a dense index_to_id Vec<NodeId> (4 bytes/node)
        // with binary search for reverse lookups when node IDs are non-dense.
        let row_entry_bytes = std::mem::size_of::<u32>();
        let column_entry_bytes = std::mem::size_of::<u32>();
        let one_direction_bytes =
            (node_count + 1) * row_entry_bytes + edge_count * column_entry_bytes;
        let compaction_bytes = node_count * std::mem::size_of::<weave_graph_core::NodeId>();
        eprintln!(
            "n={node_count} m={edge_count}: csr core (load, forward only) ~{:.1} bytes/node, ~{:.1} bytes/edge; \
             if callers_within is ever called (forward+reverse) ~{:.1} bytes/node, ~{:.1} bytes/edge; \
             +compaction index ~{:.1} bytes/node",
            one_direction_bytes as f64 / node_count as f64,
            (edge_count * column_entry_bytes) as f64 / edge_count.max(1) as f64,
            (one_direction_bytes * 2) as f64 / node_count as f64,
            (edge_count * column_entry_bytes * 2) as f64 / edge_count.max(1) as f64,
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
