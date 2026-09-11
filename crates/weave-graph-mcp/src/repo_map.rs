use std::collections::HashMap;

use weave_graph_core::modules::{aggregate_file_edges, build_modules};
use weave_graph_core::{CsrGraph, NodeId, Storage};

use crate::tools::{RepoMapArgs, RepoMapResult};

/// Progressive architectural orientation (~200 tokens).
/// File-level mode groups nodes by file and ranks files by outbound
/// degree; module-level mode (`args.module == Some(true)`) folds the
/// file-dependency graph into Louvain modules first (M2.9) — one line
/// per module, drill-down into files via `weave_file_api` unchanged.
pub fn weave_repo_map(storage: &dyn Storage, csr: &CsrGraph, args: RepoMapArgs) -> RepoMapResult {
    let nodes = match storage.all_nodes() {
        Ok(n) => n,
        Err(e) => {
            return RepoMapResult {
                text: format!("error: {e}"),
            };
        }
    };

    if args.module.unwrap_or(false) {
        return module_map(storage, &nodes, args);
    }

    // Count outbound edges per file by summing each node's degree.
    let mut file_degree: HashMap<&str, usize> = HashMap::new();
    let mut file_symbol_count: HashMap<&str, usize> = HashMap::new();
    for node in &nodes {
        let degree = csr.outbound(node.id).len();
        *file_degree.entry(node.path.as_str()).or_insert(0) += degree;
        *file_symbol_count.entry(node.path.as_str()).or_insert(0) += 1;
    }

    let mut files: Vec<(&str, usize, usize)> = file_degree
        .iter()
        .map(|(&path, &deg)| (path, deg, file_symbol_count[path]))
        .collect();
    // Sort descending by degree (hub nodes first), then by path for stability.
    files.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));

    // M2.16: with `max_tokens` set, truncation is token-estimate-based —
    // lines are added while the running estimate fits. Without it,
    // `max_files` truncation is byte-identical to today.
    let mut lines = Vec::with_capacity(files.len() + 1);
    lines.push(format!(
        "repo map ({} files, {} total):",
        files.len().min(args.max_files),
        file_symbol_count.len()
    ));
    let mut rendered: Vec<String> = files
        .iter()
        .map(|(path, degree, symbols)| format!("  {path}  [{symbols} symbols, {degree} edges]"))
        .collect();
    if let Some(max) = args.max_tokens {
        let mut kept: Vec<String> = Vec::new();
        for line in rendered.drain(..) {
            if kept.len() >= args.max_files {
                break;
            }
            let mut candidate = kept.clone();
            candidate.push(line.clone());
            // The estimate covers the whole response, header included —
            // the header alone already costs tokens.
            let text = format!(
                "repo map ({} files, {} total):\n{}",
                candidate.len(),
                file_symbol_count.len(),
                candidate.join("\n")
            );
            if crate::tools::estimate_tokens(&text) > max {
                break;
            }
            kept.push(line);
        }
        return RepoMapResult {
            text: format!(
                "repo map ({} files, {} total):\n{}",
                kept.len(),
                file_symbol_count.len(),
                kept.join("\n")
            ),
        };
    }
    files.truncate(args.max_files);
    for (path, degree, symbols) in &files {
        lines.push(format!("  {path}  [{symbols} symbols, {degree} edges]"));
    }

    RepoMapResult {
        text: lines.join("\n"),
    }
}

/// Module-level orientation: one line per Louvain module — label, file
/// count, symbol count, cross-module edge weight, member files. No
/// truncation: module membership covers 100% of indexed files.
fn module_map(
    storage: &dyn Storage,
    nodes: &[weave_graph_core::Node],
    _args: RepoMapArgs,
) -> RepoMapResult {
    let edges = match storage.all_edges() {
        Ok(e) => e,
        Err(e) => {
            return RepoMapResult {
                text: format!("error: {e}"),
            };
        }
    };

    let file_of: HashMap<NodeId, String> = nodes.iter().map(|n| (n.id, n.path.clone())).collect();
    let file_edges = aggregate_file_edges(&edges, &file_of);
    let modules = build_modules(nodes, &file_edges);

    let mut file_symbols: HashMap<&str, usize> = HashMap::new();
    for node in nodes {
        *file_symbols.entry(node.path.as_str()).or_insert(0) += 1;
    }

    let total_symbols: usize = file_symbols.values().sum();
    let mut lines = vec![format!(
        "repo map ({} modules, {} files, {total_symbols} symbols):",
        modules.len(),
        file_symbols.len()
    )];
    for module in &modules {
        // Cross-module weight: file-level edges with exactly one endpoint
        // in this module (in + out together; the pair key is unordered).
        let members: std::collections::HashSet<&str> =
            module.files.iter().map(String::as_str).collect();
        let cross: f64 = file_edges
            .iter()
            .filter(|((a, b), _)| members.contains(a.as_str()) != members.contains(b.as_str()))
            .map(|(_, w)| w)
            .sum();
        let symbols: usize = module
            .files
            .iter()
            .map(|f| file_symbols.get(f.as_str()).copied().unwrap_or(0))
            .sum();
        lines.push(format!(
            "  {}  [{} files, {symbols} symbols, {} cross-edges]: {}",
            module.label,
            module.files.len(),
            cross as usize,
            module.files.join(", ")
        ));
    }

    RepoMapResult {
        text: lines.join("\n"),
    }
}

#[cfg(test)]
mod tests;
