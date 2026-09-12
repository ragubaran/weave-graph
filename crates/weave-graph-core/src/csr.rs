use std::collections::HashMap;

use petgraph::Directed;
use petgraph::csr::Csr;
use roaring::RoaringBitmap;

use crate::error::StorageError;
use crate::model::NodeId;
use crate::storage::Storage;

/// Read-only, integer-compacted adjacency structure (`plan.md` §1.1).
/// **The SQL store is authoritative; this is rebuilt from it on every
/// load — there is no path to sync a `CsrGraph` mutation back to SQL.**
/// Don't build one.
pub struct CsrGraph {
    csr: Csr<(), f64, Directed, u32>,
    id_to_index: HashMap<NodeId, u32>,
    index_to_id: Vec<NodeId>,
}

impl CsrGraph {
    // Infallible empty graph constructor for default/WASM initialization.
    // Bypasses edge sorting since vertex and edge sets are empty.
    pub fn empty() -> Self {
        Self {
            csr: Csr::new(),
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

        let mut compact_edges: Vec<(u32, u32, f64)> = Vec::new();
        storage.for_each_edge(&mut |edge| {
            let (Some(&u), Some(&v)) = (
                id_to_index.get(&edge.source_id),
                id_to_index.get(&edge.target_id),
            ) else {
                // A dangling edge endpoint is exactly the defect M1.4 exists
                // to prevent at write time; a read-side rebuild stays
                // correct by skipping it rather than panicking.
                return;
            };
            compact_edges.push((u, v, edge.weight));
        })?;
        compact_edges.sort_by_key(|&(u, v, _)| (u, v));
        compact_edges.dedup_by(|a, b| {
            let same_pair = a.0 == b.0 && a.1 == b.1;
            if same_pair {
                b.2 = b.2.max(a.2);
            }
            same_pair
        });

        // `from_sorted_edges` is O(V+E); building via repeated `add_edge`
        // is O(V·E) per petgraph's own docs — the difference is the gap
        // between this loading in milliseconds vs. minutes at the 500k
        // symbol scale `plan.md`'s resource envelope targets.
        let mut csr: Csr<(), f64, Directed, u32> = Csr::from_sorted_edges(&compact_edges)
            .map_err(|_| StorageError::Backend("CSR edges were not sorted/deduped".to_string()))?;
        // `from_sorted_edges` sizes the CSR to `max_node_id + 1`; nodes
        // with no edges at all past the highest connected id still need
        // to exist.
        while csr.node_count() < index_to_id.len() {
            csr.add_node(());
        }

        Ok(Self {
            csr,
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

        let mut compact_edges: Vec<(u32, u32, f64)> = Vec::with_capacity(edges.len());
        for &(source, target, weight) in edges {
            let (Some(&u), Some(&v)) = (id_to_index.get(&source), id_to_index.get(&target)) else {
                continue;
            };
            compact_edges.push((u, v, weight));
        }
        compact_edges.sort_by_key(|&(u, v, _)| (u, v));
        compact_edges.dedup_by(|a, b| {
            let same_pair = a.0 == b.0 && a.1 == b.1;
            if same_pair {
                b.2 = b.2.max(a.2);
            }
            same_pair
        });

        let mut csr: Csr<(), f64, Directed, u32> = Csr::from_sorted_edges(&compact_edges)
            .map_err(|_| StorageError::Backend("CSR edges were not sorted/deduped".to_string()))?;
        while csr.node_count() < index_to_id.len() {
            csr.add_node(());
        }

        Ok(Self {
            csr,
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
    /// set (`plan.md` §1.1: roaring for traversal set operations) instead
    /// of a generic hash set. `Ok(Some(path))` includes both endpoints.
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

    /// Every node reachable within `max_hops` outbound steps of `from`,
    /// as a `RoaringBitmap` of compact indices — the impact-radius
    /// building block. Callers combine radii from multiple starting
    /// points with `&`/`|` instead of hand-rolled bitmask code.
    pub fn reachable_within(&self, from: NodeId, max_hops: u32) -> RoaringBitmap {
        let mut reached = RoaringBitmap::new();
        let Some(&from_index) = self.id_to_index.get(&from) else {
            return reached;
        };
        reached.insert(from_index);
        let mut frontier = vec![from_index];

        for _ in 0..max_hops {
            let mut next = Vec::new();
            for &node in &frontier {
                for &neighbor in self.csr.neighbors_slice(node) {
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
