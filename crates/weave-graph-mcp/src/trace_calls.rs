use weave_graph_core::{CsrGraph, Node, NodeId, Storage};

use crate::tools::{TraceCallsArgs, TraceCallsResult, resolve_symbol};

/// Call chain traversal — both outgoing (calls) and incoming (callers).
/// Visited sets on both BFS walks prevent infinite loops on cyclic graphs.
///
/// `mask` is the query-layer RBAC hook (`weave_graph_core::rbac`). When
/// active, this takes the original path: materialize every node, mask
/// the whole list once, then resolve and render against that — masking
/// must happen *before* resolution (Core Invariant 7), or resolving a
/// hidden symbol by its exact real name and masking the result only
/// afterward would let a successful resolution alone leak that the
/// symbol exists. Without a mask there's no such ordering constraint, so
/// the unmasked path resolves via `get_node_by_symbol` and renders via
/// per-id `storage.get_node` lookups instead (PERF-G16): no
/// `all_nodes()` call at all for the common case.
pub fn weave_trace_calls(
    storage: &dyn Storage,
    csr: &CsrGraph,
    args: TraceCallsArgs<'_>,
    mask: Option<&dyn Fn(&Node) -> Node>,
) -> TraceCallsResult {
    match mask {
        Some(m) => weave_trace_calls_masked(storage, csr, args, m),
        None => weave_trace_calls_unmasked(storage, csr, args),
    }
}

fn weave_trace_calls_masked(
    storage: &dyn Storage,
    csr: &CsrGraph,
    args: TraceCallsArgs<'_>,
    mask: &dyn Fn(&Node) -> Node,
) -> TraceCallsResult {
    let nodes = match storage.all_nodes() {
        Ok(n) => n,
        Err(e) => {
            return TraceCallsResult {
                text: format!("error: {e}"),
            };
        }
    };
    let nodes: Vec<Node> = nodes.iter().map(mask).collect();

    let root_id = match resolve_symbol(&nodes, args.symbol) {
        Ok(id) => id,
        Err(suggestions) => {
            return TraceCallsResult {
                text: weave_graph_core::resolve::format_not_found(args.symbol, &suggestions),
            };
        }
    };

    let lookup = |id: NodeId| nodes.iter().find(|n| n.id == id).cloned();
    let outgoing = outgoing_chain(csr, &lookup, root_id, args.depth);
    let incoming = incoming_chain(storage, &lookup, root_id, args.depth);
    render_result(args, outgoing, incoming)
}

fn weave_trace_calls_unmasked(
    storage: &dyn Storage,
    csr: &CsrGraph,
    args: TraceCallsArgs<'_>,
) -> TraceCallsResult {
    let root_id = match storage.get_node_by_symbol(args.symbol) {
        Ok(Some(node)) => node.id,
        _ => {
            let nodes = match storage.all_nodes() {
                Ok(n) => n,
                Err(e) => {
                    return TraceCallsResult {
                        text: format!("error: {e}"),
                    };
                }
            };
            match resolve_symbol(&nodes, args.symbol) {
                Ok(id) => id,
                Err(suggestions) => {
                    return TraceCallsResult {
                        text: weave_graph_core::resolve::format_not_found(
                            args.symbol,
                            &suggestions,
                        ),
                    };
                }
            }
        }
    };

    let lookup = |id: NodeId| storage.get_node(id).ok().flatten();
    let outgoing = outgoing_chain(csr, &lookup, root_id, args.depth);
    let incoming = incoming_chain(storage, &lookup, root_id, args.depth);
    render_result(args, outgoing, incoming)
}

fn render_result(
    args: TraceCallsArgs<'_>,
    outgoing: Vec<String>,
    incoming: Vec<String>,
) -> TraceCallsResult {
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
    // No budget → byte-identical to today. Over budget → truncate
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
/// `lookup` resolves a `NodeId` to its display `Node`, either a per-id
/// storage read (unmasked) or a linear scan of an already-masked list.
fn outgoing_chain(
    csr: &CsrGraph,
    lookup: &dyn Fn(NodeId) -> Option<Node>,
    from: NodeId,
    depth: u32,
) -> Vec<String> {
    csr.reachable_within(from, depth)
        .iter()
        .filter_map(|idx| csr.id_of_index(idx))
        .filter(|&id| id != from)
        .filter_map(lookup)
        .map(|n| format!("{} ({}:{})", n.symbol, n.path, n.line_start))
        .collect()
}

/// BFS incoming hops via SQL get_callers (uses idx_edges_target index).
/// Tracks visited NodeIds to prevent cycles.
fn incoming_chain(
    storage: &dyn Storage,
    lookup: &dyn Fn(NodeId) -> Option<Node>,
    root: NodeId,
    depth: u32,
) -> Vec<String> {
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
            if let Some(node) = lookup(edge.source_id) {
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
