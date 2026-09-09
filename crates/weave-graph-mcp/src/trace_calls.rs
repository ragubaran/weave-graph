use weave_graph_core::{CsrGraph, Node, NodeId, Storage};

use crate::tools::{TraceCallsArgs, TraceCallsResult, resolve_symbol};

/// Call chain traversal — both outgoing (calls) and incoming (callers).
/// Visited sets on both BFS walks prevent infinite loops on cyclic graphs.
pub fn weave_trace_calls(
    storage: &dyn Storage,
    csr: &CsrGraph,
    args: TraceCallsArgs<'_>,
) -> TraceCallsResult {
    let nodes = match storage.all_nodes() {
        Ok(n) => n,
        Err(e) => {
            return TraceCallsResult {
                text: format!("error: {e}"),
            };
        }
    };

    let Some(root_id) = resolve_symbol(&nodes, args.symbol) else {
        return TraceCallsResult {
            text: format!("symbol not found: {}", args.symbol),
        };
    };

    let outgoing = outgoing_chain(csr, &nodes, root_id, args.depth);
    let incoming = incoming_chain(storage, &nodes, root_id, args.depth);

    let mut lines = vec![format!("trace_calls: {}", args.symbol)];
    lines.push(format!("  outgoing ({}):", outgoing.len()));
    lines.extend(outgoing.iter().map(|s| format!("    → {s}")));
    lines.push(format!("  incoming ({}):", incoming.len()));
    lines.extend(incoming.iter().map(|s| format!("    ← {s}")));

    TraceCallsResult {
        text: lines.join("\n"),
    }
}

/// BFS outgoing hops up to `depth` via CSR (RoaringBitmap visited set).
fn outgoing_chain(csr: &CsrGraph, nodes: &[Node], from: NodeId, depth: u32) -> Vec<String> {
    csr.reachable_within(from, depth)
        .iter()
        .filter(|&idx| {
            // reachable_within returns compact indices; map back through node list
            nodes
                .get(idx as usize)
                .map(|n| n.id != from)
                .unwrap_or(false)
        })
        .filter_map(|idx| nodes.get(idx as usize))
        .map(|n| format!("{} ({}:{})", n.symbol, n.path, n.line_start))
        .collect()
}

/// BFS incoming hops via SQL get_callers (uses idx_edges_target index).
/// Tracks visited NodeIds to prevent cycles.
fn incoming_chain(storage: &dyn Storage, nodes: &[Node], root: NodeId, depth: u32) -> Vec<String> {
    use std::collections::{HashSet, VecDeque};

    let mut visited: HashSet<NodeId> = HashSet::from([root]);
    let mut queue: VecDeque<(NodeId, u32)> = VecDeque::from([(root, 0)]);
    let mut result = Vec::new();

    while let Some((current, hop)) = queue.pop_front() {
        if hop >= depth {
            continue;
        }
        let callers = match storage.get_callers(current) {
            Ok(edges) => edges,
            Err(_) => continue,
        };
        for edge in callers {
            if !visited.insert(edge.source_id) {
                continue;
            }
            if let Some(node) = nodes.iter().find(|n| n.id == edge.source_id) {
                result.push(format!(
                    "{} ({}:{})",
                    node.symbol, node.path, node.line_start
                ));
                queue.push_back((edge.source_id, hop + 1));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests;
