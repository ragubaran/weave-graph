use std::collections::HashMap;

use roaring::RoaringBitmap;

use crate::error::StorageError;
use crate::model::NodeId;
use crate::storage::Storage;

/// Read-only, integer-compacted adjacency structure. **The SQL store is
/// authoritative; this is rebuilt from it on every load — there is no path
/// to sync a `CsrGraph` mutation back to SQL.** Don't build one.
///
/// Holds only forward CSR adjacency between calls. Caller traversals build a
/// temporary reverse view so a one-off query cannot permanently raise RSS.
pub struct CsrGraph {
    csr: CompactCsr,
    index_to_id: Vec<NodeId>,
}

/// Sorts and dedups `(u, v)` pairs in place — the one preparation step
/// `Csr::from_sorted_edges` requires.
fn sort_and_dedup_edges(edges: &mut Vec<(u32, u32)>) {
    edges.sort_unstable();
    edges.dedup();
}

#[derive(Debug, Clone)]
pub struct CompactCsr {
    row_offsets: Vec<usize>,
    column_indices: Vec<u32>,
}

impl CompactCsr {
    pub fn new() -> Self {
        Self {
            row_offsets: vec![0],
            column_indices: Vec::new(),
        }
    }

    pub fn from_sorted_edges(edges: &[(u32, u32)], node_count: usize) -> Self {
        let mut row_offsets = vec![0; node_count + 1];
        let mut column_indices = Vec::with_capacity(edges.len());

        let mut current_node = 0;
        for &(u, v) in edges {
            while current_node < u {
                current_node += 1;
                row_offsets[current_node as usize] = column_indices.len();
            }
            column_indices.push(v);
        }
        while current_node < node_count as u32 {
            current_node += 1;
            row_offsets[current_node as usize] = column_indices.len();
        }

        Self {
            row_offsets,
            column_indices,
        }
    }

    pub fn neighbors_slice(&self, u: u32) -> &[u32] {
        if (u as usize) + 1 >= self.row_offsets.len() {
            return &[];
        }
        let start = self.row_offsets[u as usize];
        let end = self.row_offsets[(u + 1) as usize];
        &self.column_indices[start..end]
    }

    pub fn node_count(&self) -> usize {
        self.row_offsets.len().saturating_sub(1)
    }

    pub fn edge_count(&self) -> usize {
        self.column_indices.len()
    }

    pub fn build_reverse(&self) -> Self {
        let n = self.node_count();
        let mut in_degrees = vec![0; n];
        for &v in &self.column_indices {
            in_degrees[v as usize] += 1;
        }

        let mut row_offsets = vec![0; n + 1];
        let mut sum = 0;
        for i in 0..n {
            row_offsets[i] = sum;
            sum += in_degrees[i];
        }
        row_offsets[n] = sum;

        let mut current_offsets = row_offsets.clone();
        let mut column_indices = vec![0; self.edge_count()];

        for u in 0..n {
            for &v in self.neighbors_slice(u as u32) {
                let pos = current_offsets[v as usize];
                column_indices[pos] = u as u32;
                current_offsets[v as usize] += 1;
            }
        }

        Self {
            row_offsets,
            column_indices,
        }
    }
}

/// Unweighted BFS from `from_index` over `csr`, bounded to `max_hops`,
/// using a `RoaringBitmap` visited set for fast union/intersection across
/// traversals. Shared by [`CsrGraph::reachable_within`]
/// (forward `csr`) and [`CsrGraph::callers_within`] (a reverse CSR) —
/// identical hop-bounding logic, different adjacency to walk.
fn bfs_within(csr: &CompactCsr, from_index: u32, max_hops: u32) -> RoaringBitmap {
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
            csr: CompactCsr::new(),
            index_to_id: Vec::new(),
        }
    }

    /// Loads every node/edge from `storage`, compacting storage ids (which
    /// may have gaps) into the dense `0..n` space `Csr` requires. Multiple
    /// edges on the same `(source, target)` pair collapse into one CSR
    /// edge carrying the max weight; kind attribution stays queryable in SQL.
    pub fn load(storage: &dyn Storage) -> Result<Self, StorageError> {
        let mut index_to_id: Vec<NodeId> = Vec::new();
        storage.for_each_node(&mut |node| {
            index_to_id.push(node.id);
        })?;
        index_to_id.sort_unstable();

        let mut compact_edges: Vec<(u32, u32)> = Vec::new();
        storage.for_each_edge(&mut |edge| {
            let Ok(u) = index_to_id.binary_search(&edge.source_id) else {
                return;
            };
            let Ok(v) = index_to_id.binary_search(&edge.target_id) else {
                return;
            };
            compact_edges.push((u as u32, v as u32));
        })?;
        sort_and_dedup_edges(&mut compact_edges);

        let csr = CompactCsr::from_sorted_edges(&compact_edges, index_to_id.len());

        Ok(Self { csr, index_to_id })
    }

    // Constructs CsrGraph directly from memory slices for WASM/offline use.
    // Preserves identical compaction, sorting, and deduping as load().
    pub fn from_nodes_and_edges(
        nodes: &[NodeId],
        edges: &[(NodeId, NodeId, f64)],
    ) -> Result<Self, StorageError> {
        let mut index_to_id: Vec<NodeId> = nodes.to_vec();
        index_to_id.sort_unstable();
        index_to_id.dedup();

        let mut compact_edges: Vec<(u32, u32)> = Vec::with_capacity(edges.len());
        for &(source, target, _weight) in edges {
            let Ok(u) = index_to_id.binary_search(&source) else {
                continue;
            };
            let Ok(v) = index_to_id.binary_search(&target) else {
                continue;
            };
            compact_edges.push((u as u32, v as u32));
        }
        sort_and_dedup_edges(&mut compact_edges);

        let csr = CompactCsr::from_sorted_edges(&compact_edges, index_to_id.len());

        Ok(Self { csr, index_to_id })
    }

    pub fn node_count(&self) -> usize {
        self.csr.node_count()
    }

    pub fn edge_count(&self) -> usize {
        self.csr.edge_count()
    }

    /// Outbound neighbor storage ids, in the order the CSR stores them.
    pub fn outbound(&self, id: NodeId) -> Vec<NodeId> {
        let Ok(index) = self.index_to_id.binary_search(&id) else {
            return Vec::new();
        };
        let index = index as u32;
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
        let from_index = self.index_to_id.binary_search(&from).ok()? as u32;
        let to_index = self.index_to_id.binary_search(&to).ok()? as u32;
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
        let Ok(from_index) = self.index_to_id.binary_search(&from) else {
            return RoaringBitmap::new();
        };
        let from_index = from_index as u32;
        bfs_within(&self.csr, from_index, max_hops)
    }

    /// Every node that reaches `from` within `max_hops` inbound steps.
    /// The reverse adjacency is temporary to keep idle graph memory bounded.
    pub fn callers_within(&self, from: NodeId, max_hops: u32) -> RoaringBitmap {
        let Ok(from_index) = self.index_to_id.binary_search(&from) else {
            return RoaringBitmap::new();
        };
        let from_index = from_index as u32;
        let reverse = self.build_reverse_csr();
        bfs_within(&reverse, from_index, max_hops)
    }

    /// Transposes forward CSR only for the active caller traversal.
    fn build_reverse_csr(&self) -> CompactCsr {
        self.csr.build_reverse()
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
