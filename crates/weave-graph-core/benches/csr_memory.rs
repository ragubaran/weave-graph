//! Bytes-per-node/edge targets for the resource envelope: ≤24 bytes/node,
//! ≤8 bytes/edge. This measures load time via criterion and reports
//! byte-size analytically rather than via runtime heap introspection —
//! `petgraph::csr::Csr`'s internal `Vec`s aren't exposed for that, and
//! adding a heap-profiling dependency just for this diagnostic isn't
//! worth it.
//!
//! Layout (`Csr<(), (), Directed, u32>`, per `petgraph::csr::Csr`'s own
//! source): `row: Vec<usize>` (len n+1 — **not** `Vec<u32>`, the row-index
//! width is fixed regardless of the `Ix` type parameter), `column:
//! Vec<NodeIndex<Ix>>` = `Vec<u32>` here (len m), `edges: Vec<()>` — zero
//! bytes, `Vec<()>` never allocates (nothing in this crate ever reads an
//! edge's weight back off the CSR, so it was dropped entirely; `Edge.weight`
//! in the SQL model is untouched). So one direction costs bytes/node ≈ 8
//! (row entry, `size_of::<usize>()`), bytes/edge ≈ 4 (column index only).
//!
//! `CsrGraph::load` (what this bench measures) only ever builds the
//! **forward** direction — `reverse_csr` is built lazily, on first
//! `callers_within` call, so a plain `load` never pays for it. The
//! "forward + reverse" row below is the cost *if and when* `callers_within`
//! is actually invoked (`weave blast --direction callers`/`both`), reported
//! alongside the load-time number for context, not because `load` itself
//! builds both. `CsrGraph`'s own id-compaction maps
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
        // `row` is `Vec<usize>` (petgraph's own field type, confirmed
        // against its source — not `Vec<u32>`/`Vec<Ix>`), so its per-entry
        // size is `size_of::<usize>()`, not a hardcoded constant. No weight
        // term: `edges: Vec<()>` costs nothing (Fix A).
        let row_entry_bytes = std::mem::size_of::<usize>();
        let column_entry_bytes = std::mem::size_of::<u32>();
        let one_direction_bytes =
            (node_count + 1) * row_entry_bytes + edge_count * column_entry_bytes;
        let compaction_bytes = node_count * (4 + 4); // id_to_index entry + index_to_id entry, ignoring HashMap load-factor overhead
        eprintln!(
            "n={node_count} m={edge_count}: csr core (load, forward only) ~{:.1} bytes/node, ~{:.1} bytes/edge; \
             if callers_within is ever called (forward+reverse) ~{:.1} bytes/node, ~{:.1} bytes/edge; \
             +compaction maps ~{:.1} bytes/node",
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
