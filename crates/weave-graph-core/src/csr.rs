use std::collections::HashMap;
use std::sync::OnceLock;

use petgraph::Directed;
use petgraph::csr::Csr;
use roaring::RoaringBitmap;

use crate::error::StorageError;
use crate::model::NodeId;
use crate::storage::Storage;

/// Read-only, integer-compacted adjacency structure. **The SQL store is
/// authoritative; this is rebuilt from it on every load — there is no path
/// to sync a `CsrGraph` mutation back to SQL.** Don't build one.
///
/// Holds the forward CSR (outbound edges — a symbol's callees) always, and
/// builds a transposed `reverse_csr` (inbound edges — its callers) lazily,
/// on the first [`callers_within`](Self::callers_within) call — every
/// consumer that never asks for callers (`weave query`/`report`/`export`,
/// every MCP tool, `weave blast --direction callees`) never builds or
/// holds it. Edges carry no weight: nothing in this crate ever reads an
/// edge's weight back off the CSR, only `neighbors_slice`'s indices —
/// carrying one was dead storage.
pub struct CsrGraph {
    csr: Csr<(), (), Directed, u32>,
    reverse_csr: OnceLock<Csr<(), (), Directed, u32>>,
    id_to_index: HashMap<NodeId, u32>,
    index_to_id: Vec<NodeId>,
}

/// Sorts and dedups `(u, v)` pairs in place — the one preparation step
/// `Csr::from_sorted_edges` requires.
fn sort_and_dedup_edges(edges: &mut Vec<(u32, u32)>) {
    edges.sort_unstable();
    edges.dedup();
}

/// Builds a `Csr` from already sorted/deduped edges, padded with isolated
/// nodes so every id up to `node_count` exists even past the highest
/// connected one (`from_sorted_edges` only sizes to `max_node_id + 1`).
fn build_csr(
    edges: &[(u32, u32)],
    node_count: usize,
) -> Result<Csr<(), (), Directed, u32>, StorageError> {
    let mut csr: Csr<(), (), Directed, u32> = Csr::from_sorted_edges(edges)
        .map_err(|_| StorageError::Backend("CSR edges were not sorted/deduped".to_string()))?;
    while csr.node_count() < node_count {
        csr.add_node(());
    }
    Ok(csr)
}

/// Unweighted BFS from `from_index` over `csr`, bounded to `max_hops`,
/// using a `RoaringBitmap` visited set for fast union/intersection across
/// traversals. Shared by [`CsrGraph::reachable_within`]
/// (forward `csr`) and [`CsrGraph::callers_within`] (`reverse_csr`) —
/// identical hop-bounding logic, different adjacency to walk.
fn bfs_within(csr: &Csr<(), (), Directed, u32>, from_index: u32, max_hops: u32) -> RoaringBitmap {
    let mut reached = RoaringBitmap::new();
    reached.insert(from_index);
    let mut frontier = vec![from_index];

    for _ in 0..max_hops {
        let mut next = Vec::new();
        for &node in &frontier {
            for &neighbor in csr.neighbors_slice(node) {
                if reached.insert(neighbor) {
                    next.push(neighbor);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    reached
}

impl CsrGraph {
    // Infallible empty graph constructor for default/WASM initialization.
    // Bypasses edge sorting since vertex and edge sets are empty.
    pub fn empty() -> Self {
        Self {
            csr: Csr::new(),
            reverse_csr: OnceLock::new(),
            id_to_index: HashMap::new(),
            index_to_id: Vec::new(),
        }
    }

    /// Loads every node/edge from `storage`, compacting storage ids (which
    /// may have gaps) into the dense `0..n` space `Csr` requires. Multiple
    /// edges on the same `(source, target)` pair collapse into one CSR
    /// edge carrying the max weight; kind attribution stays queryable in SQL.
    pub fn load(storage: &dyn Storage) -> Result<Self, StorageError> {
        // Streamed via `for_each_node`/`for_each_edge`, not `all_nodes`/
        // `all_edges` — a `Vec<Node>`/`Vec<Edge>` intermediate (five owned
        // `String` fields per node, one per edge) was the real RAM cost
        // behind the measured Core Invariant 4 violation at 500k symbols,
        // not this struct's own compact `u32`/`f64` layout below.
        let mut id_to_index: HashMap<NodeId, u32> = HashMap::new();
        let mut index_to_id: Vec<NodeId> = Vec::new();
        storage.for_each_node(&mut |node| {
            id_to_index.insert(node.id, index_to_id.len() as u32);
            index_to_id.push(node.id);
        })?;

        let mut compact_edges: Vec<(u32, u32)> = Vec::new();
        storage.for_each_edge(&mut |edge| {
            let (Some(&u), Some(&v)) = (
                id_to_index.get(&edge.source_id),
                id_to_index.get(&edge.target_id),
            ) else {
                // A dangling edge endpoint is exactly the defect
                // incremental-reindex write-time purging exists to
                // prevent; a read-side rebuild stays correct by skipping
                // it rather than panicking.
                return;
            };
            compact_edges.push((u, v));
        })?;
        sort_and_dedup_edges(&mut compact_edges);

        // `from_sorted_edges` is O(V+E); building via repeated `add_edge`
        // is O(V·E) per petgraph's own docs — the difference is the gap
        // between this loading in milliseconds vs. minutes at 500k-symbol
        // scale.
        let csr = build_csr(&compact_edges, index_to_id.len())?;

        Ok(Self {
            csr,
            reverse_csr: OnceLock::new(),
            id_to_index,
            index_to_id,
        })
    }

    // Constructs CsrGraph directly from memory slices for WASM/offline use.
    // Preserves identical compaction, sorting, and deduping as load().
    pub fn from_nodes_and_edges(
        nodes: &[NodeId],
        edges: &[(NodeId, NodeId, f64)],
    ) -> Result<Self, StorageError> {
        let mut id_to_index: HashMap<NodeId, u32> = HashMap::with_capacity(nodes.len());
        let mut index_to_id: Vec<NodeId> = Vec::with_capacity(nodes.len());
        for &id in nodes {
            if let std::collections::hash_map::Entry::Vacant(e) = id_to_index.entry(id) {
                e.insert(index_to_id.len() as u32);
                index_to_id.push(id);
            }
        }

        // `weight` is part of this function's public signature for
        // caller convenience (WASM callers often already have it on hand
        // from a JS-side edge array) but is never stored — see the type's
        // own doc comment for why.
        let mut compact_edges: Vec<(u32, u32)> = Vec::with_capacity(edges.len());
        for &(source, target, _weight) in edges {
            let (Some(&u), Some(&v)) = (id_to_index.get(&source), id_to_index.get(&target)) else {
                continue;
            };
            compact_edges.push((u, v));
        }
        sort_and_dedup_edges(&mut compact_edges);

        let csr = build_csr(&compact_edges, index_to_id.len())?;

        Ok(Self {
            csr,
            reverse_csr: OnceLock::new(),
            id_to_index,
            index_to_id,
        })
    }

    pub fn node_count(&self) -> usize {
        self.csr.node_count()
    }

    pub fn edge_count(&self) -> usize {
        self.csr.edge_count()
    }

    /// Outbound neighbor storage ids, in the order the CSR stores them.
    pub fn outbound(&self, id: NodeId) -> Vec<NodeId> {
        let Some(&index) = self.id_to_index.get(&id) else {
            return Vec::new();
        };
        self.csr
            .neighbors_slice(index)
            .iter()
            .map(|&n| self.index_to_id[n as usize])
            .collect()
    }

    /// Unweighted BFS over outbound edges, using a `RoaringBitmap` visited
    /// set for fast union/intersection with other traversal results
    /// instead of a generic hash set. `Ok(Some(path))` includes both
    /// endpoints.
    pub fn query_path(&self, from: NodeId, to: NodeId) -> Option<Vec<NodeId>> {
        let from_index = *self.id_to_index.get(&from)?;
        let to_index = *self.id_to_index.get(&to)?;
        if from_index == to_index {
            return Some(vec![from]);
        }

        let mut visited = RoaringBitmap::new();
        visited.insert(from_index);
        let mut predecessor: HashMap<u32, u32> = HashMap::new();
        let mut queue = std::collections::VecDeque::from([from_index]);

        while let Some(current) = queue.pop_front() {
            for &neighbor in self.csr.neighbors_slice(current) {
                if visited.contains(neighbor) {
                    continue;
                }
                visited.insert(neighbor);
                predecessor.insert(neighbor, current);
                if neighbor == to_index {
                    let mut path = vec![to_index];
                    let mut cur = current;
                    while cur != from_index {
                        path.push(cur);
                        cur = predecessor[&cur];
                    }
                    path.push(from_index);
                    path.reverse();
                    return Some(
                        path.into_iter()
                            .map(|i| self.index_to_id[i as usize])
                            .collect(),
                    );
                }
                queue.push_back(neighbor);
            }
        }
        None
    }

    /// Every node reachable within `max_hops` outbound steps of `from`
    /// (its transitive callees), as a `RoaringBitmap` of compact indices —
    /// the impact-radius building block. Callers combine radii from
    /// multiple starting points with `&`/`|` instead of hand-rolled
    /// bitmask code.
    pub fn reachable_within(&self, from: NodeId, max_hops: u32) -> RoaringBitmap {
        let Some(&from_index) = self.id_to_index.get(&from) else {
            return RoaringBitmap::new();
        };
        bfs_within(&self.csr, from_index, max_hops)
    }

    /// Every node that reaches `from` within `max_hops` inbound steps
    /// (its transitive callers) — depth-bounded caller traversal, walking a
    /// `reverse_csr` built lazily on first call rather than unconditionally
    /// at load time so callers-only workloads never pay to build or hold
    /// it. `Storage::get_callers`'s SQL path answers the same question
    /// unbounded; this is the CSR-side, depth-bounded counterpart
    /// `weave blast --direction callers`/`both` needs.
    pub fn callers_within(&self, from: NodeId, max_hops: u32) -> RoaringBitmap {
        let Some(&from_index) = self.id_to_index.get(&from) else {
            return RoaringBitmap::new();
        };
        let reverse = self.reverse_csr.get_or_init(|| self.build_reverse_csr());
        bfs_within(reverse, from_index, max_hops)
    }

    /// Transposes the already-built forward `csr` by walking its own
    /// adjacency once, rather than retaining a separate edge list just for
    /// this — callers that never reach `callers_within` never carry that
    /// cost either. `reverse_edges` is derived from an already-valid `csr`
    /// and sorted/deduped immediately below, so `build_csr` cannot actually
    /// fail here; falling back to an empty graph (never reachable in
    /// practice) keeps this infallible without a library-code panic path.
    fn build_reverse_csr(&self) -> Csr<(), (), Directed, u32> {
        let mut reverse_edges: Vec<(u32, u32)> = Vec::with_capacity(self.csr.edge_count());
        for u in 0..self.csr.node_count() as u32 {
            for &v in self.csr.neighbors_slice(u) {
                reverse_edges.push((v, u));
            }
        }
        sort_and_dedup_edges(&mut reverse_edges);
        build_csr(&reverse_edges, self.index_to_id.len()).unwrap_or_else(|_| Csr::new())
    }

    // Resolves internal compact CSR index back to original external NodeId.
    // Handles sparse storage IDs without exposing index renumbering details.
    pub fn id_of_index(&self, index: u32) -> Option<NodeId> {
        self.index_to_id.get(index as usize).copied()
    }

    // Resolves reachable NodeIds within max_hops steps from start node.
    // Maps internal RoaringBitmap indices back to external storage IDs.
    pub fn reachable_nodes(&self, from: NodeId, max_hops: u32) -> Vec<NodeId> {
        self.reachable_within(from, max_hops)
            .iter()
            .filter_map(|idx| self.id_of_index(idx))
            .collect()
    }
}

impl Default for CsrGraph {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests;
