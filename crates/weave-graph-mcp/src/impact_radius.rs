use weave_graph_core::{CsrGraph, Node, Storage};

use crate::tools::{ImpactRadiusArgs, ImpactRadiusResult, resolve_symbol};

/// Topological blast radius for a proposed change to `symbol`.
/// Returns every outbound-reachable node (full BFS, no depth cap).
/// CSR `reachable_within` uses a RoaringBitmap visited set — cycle-safe.
pub fn weave_impact_radius(
    storage: &dyn Storage,
    csr: &CsrGraph,
    args: ImpactRadiusArgs<'_>,
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
    let mut lines = vec![format!(
        "impact_radius: {} ({count} symbols affected)",
        args.symbol
    )];
    for node in impacted.iter().take(20) {
        lines.push(format!(
            "  {} ({}:{})",
            node.symbol, node.path, node.line_start
        ));
    }
    if count > 20 {
        lines.push(format!("  ... and {} more", count - 20));
    }

    ImpactRadiusResult {
        symbol_count: count,
        text: lines.join("\n"),
    }
}

#[cfg(test)]
mod tests;
