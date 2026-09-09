use std::collections::{HashSet, VecDeque};

use weave_graph_core::{CsrGraph, Node, NodeId, Storage};

const USAGE: &str =
    "Supported forms: callers(<symbol>), callees(<symbol>), impact(<symbol>), path(<a>,<b>)";

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

/// `weave query "<expression>"` (`plan.md` §1.3's `weave query
/// "callers(AuthService.verify)"`): a small, deterministic query language
/// over the already-indexed graph — no LLM, no network, same guarantee as
/// the MCP tools this mirrors (`weave_trace_calls`, `weave_impact_radius`).
pub(crate) fn run(storage: &dyn Storage, expression: &str) -> Result<String, String> {
    let expr = expression.trim();
    let (name, args) =
        parse_call(expr).ok_or_else(|| format!("unrecognized query: {expr}. {USAGE}"))?;
    let nodes = storage.all_nodes().map_err(|e| e.to_string())?;

    match name {
        "callers" => {
            let root = resolve(&nodes, single_arg(&args)?)?;
            Ok(callers_text(storage, &nodes, root))
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
/// set) — via `Storage::get_callers`, since the CSR only walks outbound.
fn callers_text(storage: &dyn Storage, nodes: &[Node], root: NodeId) -> String {
    let mut visited = HashSet::from([root]);
    let mut queue = VecDeque::from([root]);
    let mut lines = Vec::new();
    while let Some(current) = queue.pop_front() {
        for edge in storage.get_callers(current).unwrap_or_default() {
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
        "no results".to_string()
    } else {
        lines.join("\n")
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
