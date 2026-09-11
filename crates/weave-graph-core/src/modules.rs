//! Architectural-module folding over the file-dependency graph — the
//! single Louvain implementation both `weave report` (M1.8) and
//! `weave_repo_map`'s module mode (M2.9) call. Lives in core because
//! CLI and MCP crates share it and must never fork the algorithm.

use std::collections::HashMap;

use crate::cluster::{CommunityId, louvain_communities};
use crate::model::{Edge, Node, NodeId};

/// One architectural module: a set of files Louvain grouped by
/// cross-file edge density, labeled by the directory most of them share.
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub id: CommunityId,
    pub label: String,
    pub files: Vec<String>,
}

/// One aggregated weight per unordered file pair — how many symbol-level
/// edges cross between the two files. Self-file edges are dropped: they
/// say nothing about *inter*-file architecture, which is what module
/// folding maps.
pub fn aggregate_file_edges(
    edges: &[Edge],
    file_of: &HashMap<NodeId, String>,
) -> HashMap<(String, String), f64> {
    let mut weights: HashMap<(String, String), f64> = HashMap::new();
    for edge in edges {
        let (Some(a), Some(b)) = (file_of.get(&edge.source_id), file_of.get(&edge.target_id))
        else {
            continue;
        };
        if a == b {
            continue;
        }
        let key = if a <= b {
            (a.clone(), b.clone())
        } else {
            (b.clone(), a.clone())
        };
        *weights.entry(key).or_insert(0.0) += 1.0;
    }
    weights
}

/// Clusters the file-dependency graph into modules via Louvain
/// (`louvain_communities`), one module per community. Isolated files
/// (no cross-file edges) form their own single-file modules so the
/// result always covers 100% of indexed files.
pub fn build_modules(nodes: &[Node], file_edges: &HashMap<(String, String), f64>) -> Vec<Module> {
    let mut files: Vec<String> = nodes.iter().map(|n| n.path.clone()).collect();
    files.sort();
    files.dedup();

    // Louvain wants integer node ids; map each distinct file path to one.
    let file_id: HashMap<&str, u32> = files
        .iter()
        .enumerate()
        .map(|(i, f)| (f.as_str(), i as u32))
        .collect();
    let id_edges: Vec<(u32, u32, f64)> = file_edges
        .iter()
        .filter_map(|((a, b), w)| Some((*file_id.get(a.as_str())?, *file_id.get(b.as_str())?, *w)))
        .collect();
    let ids: Vec<u32> = (0..files.len() as u32).collect();
    let communities = louvain_communities(&ids, &id_edges);

    let mut by_module: HashMap<CommunityId, Vec<String>> = HashMap::new();
    for (i, file) in files.iter().enumerate() {
        let community = communities.get(&(i as u32)).copied().unwrap_or(0);
        by_module.entry(community).or_default().push(file.clone());
    }

    let mut modules: Vec<Module> = by_module
        .into_iter()
        .map(|(id, mut files)| {
            files.sort();
            let label = module_label(&files);
            Module { id, label, files }
        })
        .collect();
    modules.sort_by(|a, b| b.files.len().cmp(&a.files.len()).then(a.id.cmp(&b.id)));
    modules
}

/// Names a module after the directory most of its files share — falls
/// back to a bare id when the files don't agree on one (mixed top-level
/// dirs, or single-file modules with no meaningful shared prefix).
fn module_label(files: &[String]) -> String {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for f in files {
        let dir = f.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        if !dir.is_empty() {
            *counts.entry(dir).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_else(|| "(root)".to_string())
}

#[cfg(test)]
mod tests;
