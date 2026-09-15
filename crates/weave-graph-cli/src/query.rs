use std::collections::{HashSet, VecDeque};

use weave_graph_core::{CsrGraph, Node, NodeId, Storage};

const USAGE: &str = "Supported forms: callers(<symbol>), callees(<symbol>), impact(<symbol>), path(<a>,<b>), latency(<symbol>)";

fn resolve_symbol(nodes: &[Node], symbol: &str) -> Option<NodeId> {
    nodes.iter().find(|n| n.symbol == symbol).map(|n| n.id)
}

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
/// `mask` is the query-layer RBAC hook (`weave_graph_core::rbac`):
/// applied once, here, to every fetched node before any lookup below
/// touches it — `Node::clone`s a visible node untouched, replaces a
/// hidden one's content with an opaque stand-in. Never drops or reorders
/// entries: `reachable_text`'s `nodes.get(idx)` assumes the same length
/// and order `CsrGraph::load` compacted its own indices from.
pub(crate) fn run(
    storage: &dyn Storage,
    expression: &str,
    mask: Option<&dyn Fn(&Node) -> Node>,
) -> Result<String, String> {
    let expr = expression.trim();
    let (name, args) =
        parse_call(expr).ok_or_else(|| format!("unrecognized query: {expr}. {USAGE}"))?;
    let nodes = storage.all_nodes().map_err(|e| e.to_string())?;
    let nodes: Vec<Node> = match mask {
        Some(m) => nodes.iter().map(m).collect(),
        None => nodes,
    };

    match name {
        "callers" => {
            let root = resolve(&nodes, single_arg(&args)?)?;
            callers_text(storage, &nodes, root)
        }
        "callees" => {
            let root = resolve(&nodes, single_arg(&args)?)?;
            let csr = CsrGraph::load(storage).map_err(|e| e.to_string())?;
            Ok(reachable_text(&csr, &nodes, root, 1))
        }
        "impact" => {
            let root = resolve(&nodes, single_arg(&args)?)?;
            let csr = CsrGraph::load(storage).map_err(|e| e.to_string())?;
            Ok(reachable_text(&csr, &nodes, root, u32::MAX))
        }
        "path" => {
            let (a, b) = pair_args(&args)?;
            let from = resolve(&nodes, a)?;
            let to = resolve(&nodes, b)?;
            let csr = CsrGraph::load(storage).map_err(|e| e.to_string())?;
            Ok(path_text(&csr, &nodes, from, to))
        }
        "latency" => {
            // Trace-span overlay, resolved against the (possibly
            // rbac-masked) node list — a hidden symbol fails resolution
            // here and never reaches the span store.
            #[cfg(feature = "otel")]
            {
                let symbol = resolve(&nodes, single_arg(&args)?)?;
                let node = nodes
                    .iter()
                    .find(|n| n.id == symbol)
                    .ok_or_else(|| format!("Symbol {} not found", symbol))?;
                crate::traces::latency_text(storage, &node.symbol)
            }
            #[cfg(not(feature = "otel"))]
            {
                let _ = (&nodes, &args);
                Err("latency() requires the `otel` feature; \
                     rebuild with `--features otel`"
                    .to_string())
            }
        }
        other => Err(format!("unknown query function '{other}'. {USAGE}")),
    }
}

fn resolve(nodes: &[Node], symbol: &str) -> Result<NodeId, String> {
    resolve_symbol(nodes, symbol).ok_or_else(|| format!("symbol not found: {symbol}"))
}

fn describe(n: &Node) -> String {
    format!("{} ({}:{})", n.symbol, n.path, n.line_start)
}

/// All transitive callers of `root` (unbounded, cycle-safe via a visited
/// set) — via `Storage::get_callers`, propagating backend storage errors.
fn callers_text(storage: &dyn Storage, nodes: &[Node], root: NodeId) -> Result<String, String> {
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
            if let Some(node) = nodes.iter().find(|n| n.id == edge.source_id) {
                lines.push(describe(node));
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

fn reachable_text(csr: &CsrGraph, nodes: &[Node], root: NodeId, max_hops: u32) -> String {
    let lines: Vec<String> = csr
        .reachable_within(root, max_hops)
        .iter()
        .filter_map(|idx| nodes.get(idx as usize))
        .filter(|n| n.id != root)
        .map(describe)
        .collect();
    if lines.is_empty() {
        "no results".to_string()
    } else {
        lines.join("\n")
    }
}

fn path_text(csr: &CsrGraph, nodes: &[Node], from: NodeId, to: NodeId) -> String {
    match csr.query_path(from, to) {
        Some(path) => path
            .iter()
            .filter_map(|id| nodes.iter().find(|n| n.id == *id))
            .map(|n| n.symbol.as_str())
            .collect::<Vec<_>>()
            .join(" → "),
        None => "no path found".to_string(),
    }
}

#[cfg(test)]
mod tests;
