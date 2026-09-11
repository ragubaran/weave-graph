use weave_graph_core::{CsrGraph, Node, NodeId, Storage};

use crate::tools::{TraceCallsArgs, TraceCallsResult, resolve_symbol};

/// Call chain traversal — both outgoing (calls) and incoming (callers).
/// Visited sets on both BFS walks prevent infinite loops on cyclic graphs.
///
/// `mask` is M3.0's query-layer RBAC hook (`weave_graph_core::rbac`),
/// applied once here to the whole node list — see
/// `weave-graph-cli::query::run`'s doc comment for why (never drops or
/// reorders entries, so `csr`'s compact indices stay valid).
pub fn weave_trace_calls(
    storage: &dyn Storage,
    csr: &CsrGraph,
    args: TraceCallsArgs<'_>,
    mask: Option<&dyn Fn(&Node) -> Node>,
) -> TraceCallsResult {
    let nodes = match storage.all_nodes() {
        Ok(n) => n,
        Err(e) => {
            return TraceCallsResult {
                text: format!("error: {e}"),
            };
        }
    };
    let nodes: Vec<Node> = match mask {
        Some(m) => nodes.iter().map(m).collect(),
        None => nodes,
    };

    let Some(root_id) = resolve_symbol(&nodes, args.symbol) else {
        return TraceCallsResult {
            text: format!("symbol not found: {}", args.symbol),
        };
    };

    let outgoing = outgoing_chain(csr, &nodes, root_id, args.depth);
    let incoming = incoming_chain(storage, &nodes, root_id, args.depth);

    let render = |per_chain: usize| -> String {
        let mut out = vec![format!("trace_calls: {}", args.symbol)];
        for (label, chain) in [("outgoing", &outgoing), ("incoming", &incoming)] {
            out.push(format!("  {} ({}):", label, chain.len()));
            out.extend(chain.iter().take(per_chain).map(|s| format!("    → {s}")));
            let hidden = chain.len().saturating_sub(per_chain);
            if hidden > 0 {
                out.push(format!("    … and {hidden} more"));
            }
        }
        out.join("\n")
    };

    let full = render(usize::MAX);
    // M2.16: no budget → byte-identical to today. Over budget → truncate
    // the chain lines (hops closest to the root survive) with explicit
    // "... and N more" markers — the totals never go silent.
    if crate::tools::under_budget(&full, args.max_tokens) {
        return TraceCallsResult { text: full };
    }
    let max = args.max_tokens.unwrap_or(0);
    let mut per_chain = outgoing.len().max(incoming.len());
    while per_chain > 0 {
        per_chain -= 1;
        let text = render(per_chain);
        if crate::tools::estimate_tokens(&text) <= max {
            return TraceCallsResult { text };
        }
    }
    TraceCallsResult { text: render(0) }
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
