use weave_graph_core::{CsrGraph, Node, NodeId, Storage};

use crate::tools::{ImpactRadiusArgs, ImpactRadiusResult, resolve_symbol};

/// Topological blast radius for a proposed change to `symbol`.
/// Returns every outbound-reachable node (full BFS, no depth cap).
/// CSR `reachable_within` uses a RoaringBitmap visited set — cycle-safe.
///
/// `mask` is the query-layer RBAC hook (`weave_graph_core::rbac`). When
/// it's active this takes the original, simple path: materialize every
/// node, mask the whole list, resolve and render against that — masking
/// must happen *before* resolution (Core Invariant 7): resolving by a
/// hidden symbol's exact real name and masking the result only
/// afterward would let a successful resolution alone leak that the
/// symbol exists, exactly the metadata side-channel RBAC exists to
/// close. When there's no mask to apply, that ordering constraint
/// doesn't exist, so the no-mask path below never materializes
/// `all_nodes()` for the common case (PERF-G16): a `get_node_by_symbol`
/// fast path for resolution, then per-id lookups for the impacted set,
/// only reaching for the full node list if the rare module-summary tier
/// is needed.
pub fn weave_impact_radius(
    storage: &dyn Storage,
    csr: &CsrGraph,
    args: ImpactRadiusArgs<'_>,
    mask: Option<&dyn Fn(&Node) -> Node>,
) -> ImpactRadiusResult {
    match mask {
        Some(m) => weave_impact_radius_masked(storage, csr, args, m),
        None => weave_impact_radius_unmasked(storage, csr, args),
    }
}

fn weave_impact_radius_masked(
    storage: &dyn Storage,
    csr: &CsrGraph,
    args: ImpactRadiusArgs<'_>,
    mask: &dyn Fn(&Node) -> Node,
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
    let nodes: Vec<Node> = nodes.iter().map(mask).collect();

    let root_id = match resolve_symbol(&nodes, args.symbol) {
        Ok(id) => id,
        Err(suggestions) => {
            return ImpactRadiusResult {
                symbol_count: 0,
                text: weave_graph_core::resolve::format_not_found(args.symbol, &suggestions),
            };
        }
    };

    let reached = csr.reachable_within(root_id, u32::MAX);
    let impacted: Vec<&Node> = reached
        .iter()
        .filter_map(|idx| nodes.get(idx as usize))
        .filter(|n| n.id != root_id)
        .collect();

    render(storage, args, root_id, &impacted, || {
        (nodes.clone(), storage.all_edges().unwrap_or_default())
    })
}

/// No RBAC guard active: never materializes `all_nodes()` for the
/// common case. `reached`'s CSR indices are mapped to `NodeId`s via
/// `csr.id_of_index`, then looked up one at a time — bounded by blast-
/// radius size, not graph size, unlike the masked path's one bulk
/// `Vec<Node>` covering every node in the graph.
fn weave_impact_radius_unmasked(
    storage: &dyn Storage,
    csr: &CsrGraph,
    args: ImpactRadiusArgs<'_>,
) -> ImpactRadiusResult {
    let root_id = match storage.get_node_by_symbol(args.symbol) {
        Ok(Some(node)) => node.id,
        _ => {
            // Fast-path miss (not found, or a storage error re-surfaced by
            // the fallback below) — only now does the fuzzy chain need to
            // see every symbol.
            let nodes = match storage.all_nodes() {
                Ok(n) => n,
                Err(e) => {
                    return ImpactRadiusResult {
                        symbol_count: 0,
                        text: format!("error: {e}"),
                    };
                }
            };
            match resolve_symbol(&nodes, args.symbol) {
                Ok(id) => id,
                Err(suggestions) => {
                    return ImpactRadiusResult {
                        symbol_count: 0,
                        text: weave_graph_core::resolve::format_not_found(
                            args.symbol,
                            &suggestions,
                        ),
                    };
                }
            }
        }
    };

    let reached = csr.reachable_within(root_id, u32::MAX);
    let impacted: Vec<Node> = reached
        .iter()
        .filter_map(|idx| csr.id_of_index(idx))
        .filter(|&id| id != root_id)
        .filter_map(|id| storage.get_node(id).ok().flatten())
        .collect();
    let impacted: Vec<&Node> = impacted.iter().collect();

    render(storage, args, root_id, &impacted, || {
        (
            storage.all_nodes().unwrap_or_default(),
            storage.all_edges().unwrap_or_default(),
        )
    })
}

/// Shared tiered rendering (full → per-file → module summary), identical
/// for both paths above — `fetch_all` is only ever called for the rare
/// module-summary tier, lazily, so the unmasked path's whole point (never
/// touching `all_nodes()` in the common case) holds even though this
/// function's signature accepts the possibility.
fn render(
    _storage: &dyn Storage,
    args: ImpactRadiusArgs<'_>,
    root_id: NodeId,
    impacted: &[&Node],
    fetch_all: impl FnOnce() -> (Vec<Node>, Vec<weave_graph_core::Edge>),
) -> ImpactRadiusResult {
    let _ = root_id;
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
    // No budget set → output stays byte-identical. Over budget → shed in
    // tiers: per-file summary, then module folding (the one Louvain
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
        for node in impacted {
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

    // Tier 2: module summary (a hub spanning hundreds of files) — the one
    // tier that genuinely needs the whole graph, fetched lazily right here.
    let (nodes, edges) = fetch_all();
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
