use std::collections::HashMap;

use weave_graph_core::{CsrGraph, Storage};

use crate::tools::{RepoMapArgs, RepoMapResult};

/// Progressive architectural orientation (~200 tokens).
/// Groups nodes by file, ranks files by outbound degree, returns a
/// compact text summary for AI agents to orient before drilling in.
pub fn weave_repo_map(storage: &dyn Storage, csr: &CsrGraph, args: RepoMapArgs) -> RepoMapResult {
    let nodes = match storage.all_nodes() {
        Ok(n) => n,
        Err(e) => {
            return RepoMapResult {
                text: format!("error: {e}"),
            };
        }
    };

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
    files.truncate(args.max_files);

    let mut lines = Vec::with_capacity(files.len() + 1);
    lines.push(format!(
        "repo map ({} files, {} total):",
        files.len(),
        file_symbol_count.len()
    ));
    for (path, degree, symbols) in &files {
        lines.push(format!("  {path}  [{symbols} symbols, {degree} edges]"));
    }

    RepoMapResult {
        text: lines.join("\n"),
    }
}

#[cfg(test)]
mod tests;
