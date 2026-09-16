//! Graph policy linting: declared
//! architectural boundaries evaluated against the indexed symbol graph,
//! plus the drift checks (cycles, orphans) that flag architecture rot
//! before deployment. Pure graph math — YAML parsing stays in the CLI
//! crate; this module never sees a file path.
//!
//! Hidden nodes are never passed in: an `rbac`-masked view's edges to
//! hidden endpoints cannot be classified, so the caller drops them
//! upstream and reports the skip count itself.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::model::{Edge, Node, NodeId};

/// One side of a boundary rule: a file-path prefix denoting a module
/// (`src/ui`, `crates/server/src`). Matched boundary-safe — `src/ui` must
/// not swallow `src/utils` — and an empty prefix matches everything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Boundary {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundaryRule {
    /// No edge may cross `from -> to`.
    Disallow(Boundary),
    /// At least one edge must cross `from -> to`.
    Require(Boundary),
}

impl BoundaryRule {
    pub fn boundary(&self) -> &Boundary {
        match self {
            BoundaryRule::Disallow(b) | BoundaryRule::Require(b) => b,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Stable name for the rule that failed (`disallow`, `require`).
    pub kind: &'static str,
    pub from: String,
    pub to: String,
    /// `caller (path) -> callee (path)` samples proving the violation.
    pub examples: Vec<String>,
}

/// Boundary-safe module membership: `path` is `prefix` itself, or lives
/// under `prefix/` — never a sibling whose name merely starts with it.
pub fn in_module(path: &str, prefix: &str) -> bool {
    path == prefix || prefix.is_empty() || path.starts_with(&format!("{prefix}/"))
}

/// Lints the graph against `rules`. Deterministic output, sorted by
/// `(kind, from, to)`; `examples` capped at 5 per violation so a CI log
/// stays readable.
pub fn lint(nodes: &[Node], edges: &[Edge], rules: &[BoundaryRule]) -> Vec<Violation> {
    let path_of: HashMap<NodeId, &str> = nodes.iter().map(|n| (n.id, n.path.as_str())).collect();

    let mut violations = Vec::new();
    for rule in rules {
        let boundary = rule.boundary();
        let crossing: Vec<(NodeId, NodeId)> = edges
            .iter()
            .filter(|e| {
                matches!(
                    (path_of.get(&e.source_id), path_of.get(&e.target_id)),
                    (Some(src), Some(dst)) if in_module(src, &boundary.from) && in_module(dst, &boundary.to),
                )
            })
            .map(|e| (e.source_id, e.target_id))
            .collect();

        let node_by_id = |id| nodes.iter().find(|n| n.id == id);
        let kind = match rule {
            BoundaryRule::Disallow(_) if !crossing.is_empty() => "disallow",
            BoundaryRule::Require(_) if crossing.is_empty() => "require",
            _ => continue,
        };
        let examples: Vec<String> = crossing
            .iter()
            .take(5)
            .filter_map(|(src, dst)| {
                let (a, b) = (node_by_id(*src)?, node_by_id(*dst)?);
                Some(format!(
                    "{} ({}) -> {} ({})",
                    a.symbol, a.path, b.symbol, b.path
                ))
            })
            .collect();
        violations.push(Violation {
            kind,
            from: boundary.from.clone(),
            to: boundary.to.clone(),
            examples,
        });
    }
    violations.sort_by(|a, b| (a.kind, &a.from, &a.to).cmp(&(b.kind, &b.from, &b.to)));
    violations
}

/// File-level dependency cycles (the drift check). Returns
/// each distinct cycle's file paths, rotated to start at its
/// lexicographically smallest file so the same ring found from different
/// entry points dedupes to one report.
pub fn find_cycles(nodes: &[Node], edges: &[Edge]) -> Vec<Vec<String>> {
    let file_of: HashMap<NodeId, &str> = nodes.iter().map(|n| (n.id, n.path.as_str())).collect();
    let mut adjacency: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for edge in edges {
        if let (Some(a), Some(b)) = (file_of.get(&edge.source_id), file_of.get(&edge.target_id))
            && a != b
        {
            adjacency.entry(a).or_default().insert(b);
        }
    }

    let mut color: HashMap<&str, u8> = HashMap::new();
    let mut stack: Vec<&str> = Vec::new();
    let mut found: BTreeSet<Vec<String>> = BTreeSet::new();
    for start in adjacency.keys() {
        dfs_cycle(start, &adjacency, &mut color, &mut stack, &mut found);
    }
    found.into_iter().map(rotate_canonical).collect()
}

type FileGraph<'a> = BTreeMap<&'a str, BTreeSet<&'a str>>;

/// 0 = unvisited, 1 = on stack, 2 = done. A back edge to a node still on
/// the stack closes a cycle, reconstructed from the current stack slice.
fn dfs_cycle<'a>(
    file: &'a str,
    adjacency: &FileGraph<'a>,
    color: &mut HashMap<&'a str, u8>,
    stack: &mut Vec<&'a str>,
    found: &mut BTreeSet<Vec<String>>,
) {
    match color.get(file) {
        Some(1) => {
            let start = stack.iter().position(|f| *f == file).unwrap_or(0);
            found.insert(
                stack[start..]
                    .iter()
                    .chain(std::iter::once(&file))
                    .map(|f| f.to_string())
                    .collect(),
            );
            return;
        }
        Some(2) => return,
        _ => {}
    }
    color.insert(file, 1);
    stack.push(file);
    for next in adjacency.get(file).into_iter().flatten() {
        dfs_cycle(next, adjacency, color, stack, found);
    }
    stack.pop();
    color.insert(file, 2);
}

fn rotate_canonical(mut cycle: Vec<String>) -> Vec<String> {
    if let Some(min_pos) = cycle
        .iter()
        .enumerate()
        .min_by_key(|(_, f)| f.as_str())
        .map(|(i, _)| i)
    {
        cycle.rotate_left(min_pos);
    }
    cycle
}

/// Files nothing depends on: zero inbound cross-file edges. Entry points
/// (`main`, scripts) show up here too — the report is advisory drift
/// signal, not a verdict.
pub fn orphan_files(nodes: &[Node], edges: &[Edge]) -> Vec<String> {
    let file_of: HashMap<NodeId, &str> = nodes.iter().map(|n| (n.id, n.path.as_str())).collect();
    let mut targeted: HashSet<&str> = HashSet::new();
    for edge in edges {
        if let (Some(a), Some(b)) = (file_of.get(&edge.source_id), file_of.get(&edge.target_id))
            && a != b
        {
            targeted.insert(b);
        }
    }
    let mut files: BTreeSet<&str> = nodes.iter().map(|n| n.path.as_str()).collect();
    for file in targeted {
        files.remove(file);
    }
    files.into_iter().map(|f| f.to_string()).collect()
}

#[cfg(test)]
mod tests;
