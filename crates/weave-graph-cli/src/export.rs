use std::collections::HashSet;

use serde::Serialize;
use weave_graph_core::{CsrGraph, Edge, Node, NodeId, Storage};

use crate::provenance::Provenance;

#[derive(Serialize)]
pub(crate) struct Neighborhood {
    root: String,
    depth: u32,
    nodes: Vec<NodeView>,
    edges: Vec<EdgeView>,
    /// `plan.md` §1.3a: every export carries a provenance badge. Filled in
    /// by the caller (`cmd_export`), which is the layer that knows `root`.
    pub(crate) provenance: Option<Provenance>,
    /// Signed doc links touching this neighborhood (`impl.md` M2.3) —
    /// empty (and omitted from the JSON) when none carry provenance, so
    /// default-build output stays byte-identical.
    #[cfg(feature = "provenance")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) doc_provenance: Vec<crate::doc_provenance::DocProvenanceView>,
}

#[derive(Serialize)]
struct NodeView {
    id: NodeId,
    symbol: String,
    kind: String,
    path: String,
    line_start: u32,
    line_end: u32,
    signature: String,
}

impl From<&Node> for NodeView {
    fn from(n: &Node) -> Self {
        Self {
            id: n.id,
            symbol: n.symbol.clone(),
            kind: n.kind.clone(),
            path: n.path.clone(),
            line_start: n.line_start,
            line_end: n.line_end,
            signature: n.signature.clone(),
        }
    }
}

#[derive(Serialize)]
struct EdgeView {
    source_id: NodeId,
    target_id: NodeId,
    kind: String,
}

impl From<&Edge> for EdgeView {
    fn from(e: &Edge) -> Self {
        Self {
            source_id: e.source_id,
            target_id: e.target_id,
            kind: e.kind.clone(),
        }
    }
}

/// `weave export --symbol <name> --depth <n>` (`plan.md` §1.3a's LOD 3,
/// materialized on demand): the symbol's `depth`-hop neighborhood in both
/// directions — callers (via `Storage::get_callers`, since the CSR only
/// walks outbound) and callees (via `CsrGraph::reachable_within`). This is
/// the raw subgraph data, not the `.canvas` visual format with LOD 0-2
/// clustering — that's M1.8's job; this milestone only wires the command.
pub(crate) fn neighborhood(
    storage: &dyn Storage,
    symbol: &str,
    depth: u32,
) -> Result<Neighborhood, String> {
    let nodes = storage.all_nodes().map_err(|e| e.to_string())?;
    let root = nodes
        .iter()
        .find(|n| n.symbol == symbol)
        .map(|n| n.id)
        .ok_or_else(|| format!("symbol not found: {symbol}"))?;

    let csr = CsrGraph::load(storage).map_err(|e| e.to_string())?;
    let mut ids: HashSet<NodeId> = csr
        .reachable_within(root, depth)
        .iter()
        .filter_map(|idx| nodes.get(idx as usize))
        .map(|n| n.id)
        .collect();
    ids.extend(inbound_within(storage, root, depth));
    ids.insert(root);

    let selected_nodes: Vec<NodeView> = nodes
        .iter()
        .filter(|n| ids.contains(&n.id))
        .map(NodeView::from)
        .collect();

    let all_edges = storage.all_edges().map_err(|e| e.to_string())?;
    let selected_edges: Vec<EdgeView> = all_edges
        .iter()
        .filter(|e| ids.contains(&e.source_id) && ids.contains(&e.target_id))
        .map(EdgeView::from)
        .collect();

    Ok(Neighborhood {
        root: symbol.to_string(),
        depth,
        nodes: selected_nodes,
        edges: selected_edges,
        provenance: None,
        #[cfg(feature = "provenance")]
        doc_provenance: Vec::new(),
    })
}

impl Neighborhood {
    /// Used only by the `provenance` feature's export wiring.
    #[cfg(feature = "provenance")]
    pub(crate) fn node_ids(&self) -> HashSet<NodeId> {
        self.nodes.iter().map(|n| n.id).collect()
    }
}

fn inbound_within(storage: &dyn Storage, root: NodeId, depth: u32) -> HashSet<NodeId> {
    let mut visited = HashSet::from([root]);
    let mut frontier = vec![root];
    for _ in 0..depth {
        let mut next = Vec::new();
        for id in &frontier {
            for edge in storage.get_callers(*id).unwrap_or_default() {
                if visited.insert(edge.source_id) {
                    next.push(edge.source_id);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    visited
}

#[cfg(test)]
mod tests;
