use weave_graph_core::{CsrGraph, Node, Storage};

use crate::tools::{ImpactRadiusArgs, ImpactRadiusResult, resolve_symbol};

/// Topological blast radius for a proposed change to `symbol`.
/// Returns every outbound-reachable node (full BFS, no depth cap).
/// CSR `reachable_within` uses a RoaringBitmap visited set — cycle-safe.
///
/// `mask` is M3.0's query-layer RBAC hook (`weave_graph_core::rbac`),
/// applied once here to the whole node list — see
/// `weave-graph-cli::query::run`'s doc comment for why.
pub fn weave_impact_radius(
    storage: &dyn Storage,
    csr: &CsrGraph,
    args: ImpactRadiusArgs<'_>,
    mask: Option<&dyn Fn(&Node) -> Node>,
) -> ImpactRadiusResult {
    let nodes = match storage.all_nodes() {
        Ok(n) => n,
        Err(e) => {
            return ImpactRadiusResult {
                symbol_count: 0,
                text: format!("error: {e}"),
            };
        }
    };
    let nodes: Vec<Node> = match mask {
        Some(m) => nodes.iter().map(m).collect(),
        None => nodes,
    };

    let Some(root_id) = resolve_symbol(&nodes, args.symbol) else {
        return ImpactRadiusResult {
            symbol_count: 0,
            text: format!("symbol not found: {}", args.symbol),
        };
    };

    // u32::MAX gives full BFS — the visited set prevents re-visiting.
    let reached = csr.reachable_within(root_id, u32::MAX);
    let impacted: Vec<&Node> = reached
        .iter()
        .filter_map(|idx| nodes.get(idx as usize))
        .filter(|n| n.id != root_id)
        .collect();

    let count = impacted.len();
    let header = format!("impact_radius: {} ({count} symbols affected)", args.symbol);

    let render_full = || -> String {
        let mut lines = vec![header.clone()];
        for node in impacted.iter().take(20) {
            lines.push(format!(
                "  {} ({}:{})",
                node.symbol, node.path, node.line_start
            ));
        }
        if count > 20 {
            lines.push(format!("  ... and {} more", count - 20));
        }
        lines.join("\n")
    };

    let full = render_full();
    // M2.16: no budget → byte-identical to today. Over budget → shed in
    // tiers: per-file summary, then module folding (M2.9's one Louvain
    // implementation) — the hub's own totals stay in the header either way.
    if crate::tools::under_budget(&full, args.max_tokens) {
        return ImpactRadiusResult {
            symbol_count: count,
            text: full,
        };
    }

    // Tier 1: per-file summary.
    let by_file: std::collections::BTreeMap<&str, usize> = {
        let mut m = std::collections::BTreeMap::new();
        for node in &impacted {
            *m.entry(node.path.as_str()).or_insert(0) += 1;
        }
        m
    };
    let render_files = || -> String {
        let mut lines = vec![format!(
            "impact_radius: {} ({count} symbols affected — shed to file summary)",
            args.symbol
        )];
        for (path, symbols) in &by_file {
            lines.push(format!("  {path}: {symbols} symbols"));
        }
        lines.join("\n")
    };
    let files_text = render_files();
    if crate::tools::under_budget(&files_text, args.max_tokens) {
        return ImpactRadiusResult {
            symbol_count: count,
            text: files_text,
        };
    }

    // Tier 2: module summary (a hub spanning hundreds of files).
    let edges = storage.all_edges().unwrap_or_default();
    let file_of: std::collections::HashMap<weave_graph_core::NodeId, String> =
        nodes.iter().map(|n| (n.id, n.path.clone())).collect();
    let file_edges = weave_graph_core::modules::aggregate_file_edges(&edges, &file_of);
    let modules = weave_graph_core::modules::build_modules(&nodes, &file_edges);
    let impacted_paths: std::collections::HashSet<&str> =
        impacted.iter().map(|n| n.path.as_str()).collect();
    let mut out = vec![format!(
        "impact_radius: {} ({count} symbols affected — shed to module summary)",
        args.symbol
    )];
    for module in &modules {
        let hit: Vec<&String> = module
            .files
            .iter()
            .filter(|f| impacted_paths.contains(f.as_str()))
            .collect();
        if hit.is_empty() {
            continue;
        }
        out.push(format!(
            "  {} [{} files impacted]: {}",
            module.label,
            hit.len(),
            hit.iter()
                .map(|f| f.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    ImpactRadiusResult {
        symbol_count: count,
        text: out.join("\n"),
    }
}

#[cfg(test)]
mod tests;
