use std::collections::{HashSet, VecDeque};

use weave_graph_core::resolve::{format_not_found, resolve_symbol};
use weave_graph_core::{CsrGraph, Node, NodeId, Storage};

const USAGE: &str = "Supported forms: callers(<symbol>), callees(<symbol>), impact(<symbol>), path(<a>,<b>), latency(<symbol>)";

fn parse_call(expr: &str) -> Option<(&str, Vec<&str>)> {
    let open = expr.find('(')?;
    if !expr.ends_with(')') {
        return None;
    }
    let name = expr[..open].trim();
    let inner = &expr[open + 1..expr.len() - 1];
    let args = if inner.trim().is_empty() {
        Vec::new()
    } else {
        inner.split(',').map(str::trim).collect()
    };
    Some((name, args))
}

fn single_arg<'a>(args: &[&'a str]) -> Result<&'a str, String> {
    match args {
        [a] => Ok(a),
        _ => Err(format!(
            "expected exactly one argument, got {}. {USAGE}",
            args.len()
        )),
    }
}

fn pair_args<'a>(args: &[&'a str]) -> Result<(&'a str, &'a str), String> {
    match args {
        [a, b] => Ok((a, b)),
        _ => Err(format!(
            "expected exactly two arguments, got {}. {USAGE}",
            args.len()
        )),
    }
}

/// `weave query "<expression>"` (e.g. `weave query
/// "callers(AuthService.verify)"`): a small, deterministic query language
/// over the already-indexed graph — no LLM, no network, same guarantee as
/// the MCP tools this mirrors (`weave_trace_calls`, `weave_impact_radius`).
///
/// `mask` is the query-layer RBAC hook (`weave_graph_core::rbac`). When
/// it's active, this takes the original path: materialize every node,
/// mask the whole list once, then resolve and render against that —
/// masking must happen *before* resolution (Core Invariant 7), or
/// resolving a hidden symbol by its exact real name and masking the
/// result only afterward would let a successful resolution alone leak
/// that the symbol exists. Without a mask there's no such ordering
/// constraint, so the unmasked path resolves via `get_node_by_symbol`
/// and renders via per-id `storage.get_node` lookups instead (PERF-G16):
/// no `all_nodes()` call at all for the common case.
pub(crate) fn run(
    storage: &dyn Storage,
    expression: &str,
    mask: Option<&dyn Fn(&Node) -> Node>,
) -> Result<String, String> {
    let expr = expression.trim();
    let (name, args) =
        parse_call(expr).ok_or_else(|| format!("unrecognized query: {expr}. {USAGE}"))?;
    match mask {
        Some(m) => run_masked(storage, name, &args, m),
        None => run_unmasked(storage, name, &args),
    }
}

fn run_masked(
    storage: &dyn Storage,
    name: &str,
    args: &[&str],
    mask: &dyn Fn(&Node) -> Node,
) -> Result<String, String> {
    let nodes = storage.all_nodes().map_err(|e| e.to_string())?;
    let nodes: Vec<Node> = nodes.iter().map(mask).collect();
    let lookup = |id: NodeId| nodes.iter().find(|n| n.id == id).cloned();

    match name {
        "callers" => {
            let root = resolve(&nodes, single_arg(args)?)?;
            callers_text(storage, &lookup, root)
        }
        "callees" => {
            let root = resolve(&nodes, single_arg(args)?)?;
            let csr = CsrGraph::load(storage).map_err(|e| e.to_string())?;
            Ok(reachable_text(&csr, &lookup, root, 1))
        }
        "impact" => {
            let root = resolve(&nodes, single_arg(args)?)?;
            let csr = CsrGraph::load(storage).map_err(|e| e.to_string())?;
            Ok(reachable_text(&csr, &lookup, root, u32::MAX))
        }
        "path" => {
            let (a, b) = pair_args(args)?;
            let from = resolve(&nodes, a)?;
            let to = resolve(&nodes, b)?;
            let csr = CsrGraph::load(storage).map_err(|e| e.to_string())?;
            Ok(path_text(&csr, &lookup, from, to))
        }
        "latency" => {
            // Trace-span overlay, resolved against the rbac-masked node
            // list — a hidden symbol fails resolution here and never
            // reaches the span store.
            #[cfg(feature = "otel")]
            {
                let symbol = resolve(&nodes, single_arg(args)?)?;
                let node = nodes
                    .iter()
                    .find(|n| n.id == symbol)
                    .ok_or_else(|| format!("Symbol {} not found", symbol))?;
                crate::traces::latency_text(storage, &node.symbol)
            }
            #[cfg(not(feature = "otel"))]
            {
                let _ = &nodes;
                Err("latency() requires the `otel` feature; \
                     rebuild with `--features otel`"
                    .to_string())
            }
        }
        other => Err(format!("unknown query function '{other}'. {USAGE}")),
    }
}

fn run_unmasked(storage: &dyn Storage, name: &str, args: &[&str]) -> Result<String, String> {
    let lookup = |id: NodeId| storage.get_node(id).ok().flatten();

    match name {
        "callers" => {
            let root = resolve_fast(storage, single_arg(args)?)?;
            callers_text(storage, &lookup, root)
        }
        "callees" => {
            let root = resolve_fast(storage, single_arg(args)?)?;
            let csr = CsrGraph::load(storage).map_err(|e| e.to_string())?;
            Ok(reachable_text(&csr, &lookup, root, 1))
        }
        "impact" => {
            let root = resolve_fast(storage, single_arg(args)?)?;
            let csr = CsrGraph::load(storage).map_err(|e| e.to_string())?;
            Ok(reachable_text(&csr, &lookup, root, u32::MAX))
        }
        "path" => {
            let (a, b) = pair_args(args)?;
            let from = resolve_fast(storage, a)?;
            let to = resolve_fast(storage, b)?;
            let csr = CsrGraph::load(storage).map_err(|e| e.to_string())?;
            Ok(path_text(&csr, &lookup, from, to))
        }
        "latency" => {
            #[cfg(feature = "otel")]
            {
                let id = resolve_fast(storage, single_arg(args)?)?;
                let node = storage
                    .get_node(id)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("Symbol {} not found", id))?;
                crate::traces::latency_text(storage, &node.symbol)
            }
            #[cfg(not(feature = "otel"))]
            {
                let _ = args;
                Err("latency() requires the `otel` feature; \
                     rebuild with `--features otel`"
                    .to_string())
            }
        }
        other => Err(format!("unknown query function '{other}'. {USAGE}")),
    }
}

fn resolve(nodes: &[Node], symbol: &str) -> Result<NodeId, String> {
    resolve_symbol(nodes, symbol).map_err(|suggestions| format_not_found(symbol, &suggestions))
}

/// Fast-path resolution with no RBAC guard active: tries the exact-match
/// index lookup first, only materializing `all_nodes()` — for the fuzzy
/// suggestion chain — on a genuine miss.
fn resolve_fast(storage: &dyn Storage, symbol: &str) -> Result<NodeId, String> {
    if let Ok(Some(node)) = storage.get_node_by_symbol(symbol) {
        return Ok(node.id);
    }
    let nodes = storage.all_nodes().map_err(|e| e.to_string())?;
    resolve(&nodes, symbol)
}

fn describe(n: &Node) -> String {
    format!("{} ({}:{})", n.symbol, n.path, n.line_start)
}

/// All transitive callers of `root` (unbounded, cycle-safe via a visited
/// set) — via `Storage::get_callers`, propagating backend storage errors.
/// `lookup` resolves a `NodeId` to its display `Node`, either a per-id
/// storage read (unmasked) or a linear scan of an already-masked list.
fn callers_text(
    storage: &dyn Storage,
    lookup: &dyn Fn(NodeId) -> Option<Node>,
    root: NodeId,
) -> Result<String, String> {
    let mut visited = HashSet::from([root]);
    let mut queue = VecDeque::from([root]);
    let mut lines = Vec::new();
    while let Some(current) = queue.pop_front() {
        let callers = storage
            .get_callers(current)
            .map_err(|e| format!("failed to read callers for node {current}: {e}"))?;
        for edge in callers {
            if !visited.insert(edge.source_id) {
                continue;
            }
            if let Some(node) = lookup(edge.source_id) {
                lines.push(describe(&node));
                queue.push_back(edge.source_id);
            }
        }
    }
    if lines.is_empty() {
        Ok("no results".to_string())
    } else {
        Ok(lines.join("\n"))
    }
}

fn reachable_text(
    csr: &CsrGraph,
    lookup: &dyn Fn(NodeId) -> Option<Node>,
    root: NodeId,
    max_hops: u32,
) -> String {
    let lines: Vec<String> = csr
        .reachable_within(root, max_hops)
        .iter()
        .filter_map(|idx| csr.id_of_index(idx))
        .filter(|&id| id != root)
        .filter_map(lookup)
        .map(|n| describe(&n))
        .collect();
    if lines.is_empty() {
        "no results".to_string()
    } else {
        lines.join("\n")
    }
}

fn path_text(
    csr: &CsrGraph,
    lookup: &dyn Fn(NodeId) -> Option<Node>,
    from: NodeId,
    to: NodeId,
) -> String {
    match csr.query_path(from, to) {
        Some(path) => path
            .iter()
            .filter_map(|&id| lookup(id))
            .map(|n| n.symbol)
            .collect::<Vec<_>>()
            .join(" → "),
        None => "no path found".to_string(),
    }
}

#[cfg(test)]
mod tests;
