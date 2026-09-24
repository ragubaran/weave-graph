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
    /// Roles exempt from this rule (POL-04). Advisory allow-list — this
    /// module only matches names against [`lint_scoped`]'s `current_roles`;
    /// authenticating who holds a role is `weave-graph-cli`'s concern.
    pub allowed_roles: Vec<String>,
    /// Reporting-only team attribution (POL-04) — surfaces in output for
    /// routing a violation to the right team; never itself a bypass.
    pub owner_role: Option<String>,
}

impl Boundary {
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            allowed_roles: Vec::new(),
            owner_role: None,
        }
    }
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
    /// POL-04's reporting-only team attribution, carried through for
    /// routing — never consulted to decide whether this violation exists.
    pub owner_role: Option<String>,
}

/// Boundary-safe module membership: `path` is `prefix` itself, or lives
/// under `prefix/` — never a sibling whose name merely starts with it.
pub fn in_module(path: &str, prefix: &str) -> bool {
    path == prefix || prefix.is_empty() || path.starts_with(&format!("{prefix}/"))
}

/// Lints the graph against `rules`, with no identity scope — equivalent to
/// [`lint_scoped`] with an empty `current_roles`, so POL-04's
/// `allowed_roles` exemption never applies. Kept as the unscoped default
/// every pre-POL-04 caller already uses.
pub fn lint(nodes: &[Node], edges: &[Edge], rules: &[BoundaryRule]) -> Vec<Violation> {
    lint_scoped(nodes, edges, rules, &[])
}

/// Lints the graph against `rules`. Deterministic output, sorted by
/// `(kind, from, to)`; `examples` capped at 5 per violation so a CI log
/// stays readable.
///
/// `current_roles` is POL-04's exemption check: a rule whose
/// `allowed_roles` shares **any** role with `current_roles` never
/// produces a violation, even when its boundary is crossed — one shared
/// role is enough, not every listed role, since a rule naming several
/// teams most naturally means "any of these may cross this boundary."
/// `owner_role` never affects this decision; it only rides along on the
/// resulting `Violation` for routing.
pub fn lint_scoped(
    nodes: &[Node],
    edges: &[Edge],
    rules: &[BoundaryRule],
    current_roles: &[String],
) -> Vec<Violation> {
    let path_of: HashMap<NodeId, &str> = nodes.iter().map(|n| (n.id, n.path.as_str())).collect();

    let mut violations = Vec::new();
    for rule in rules {
        let boundary = rule.boundary();
        if !boundary.allowed_roles.is_empty()
            && boundary
                .allowed_roles
                .iter()
                .any(|role| current_roles.contains(role))
        {
            continue;
        }
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
            owner_role: boundary.owner_role.clone(),
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

/// POL-02: an opt-in, advisory-only rule — high embedding similarity
/// between two files that declare no relationship to each other. Never
/// evaluated by [`lint`]/[`lint_scoped`] (which block CI); only
/// `weave policy drift` surfaces these, per the same reasoning
/// `docs/proposal-skylos.md` §6.4 gives for keeping it out of the
/// hard-fail path until a false-positive-rate evaluation exists.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticCouplingRule {
    /// Path prefix both sides of a flagged pair must fall under.
    pub within: String,
    /// Minimum approximate cosine similarity (see
    /// `weave-graph-store-sqlite::vector::find_similar_pairs`'s own
    /// `INT8_UNIT_SCALE` for how this number is actually derived).
    pub threshold: f32,
    /// Files matching any of these are never flagged, on either side of a pair.
    pub exempt_globs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticCouplingFinding {
    pub file_a: String,
    pub file_b: String,
    pub similarity: f32,
}

/// Same `prefix/**`-only glob shape `weave-graph-cli::verify`'s own
/// `glob_matches` uses for `phantom_symbol_exempt_globs` — duplicated
/// rather than imported (that one lives in a different crate, gated
/// behind `federation`, unrelated to this rule's own `vector`/`policy-lint`
/// gating).
fn glob_matches(pattern: &str, path: &str) -> bool {
    match pattern.strip_suffix("/**") {
        Some(prefix) => path == prefix || path.starts_with(&format!("{prefix}/")),
        None => pattern == path,
    }
}

/// Cross-references `pairs` (already-computed similarity scores, from
/// `Storage::find_similar_node_pairs`) against `nodes`/`edges`: a pair is
/// a finding only when both files fall under `rule.within`, neither
/// matches an `exempt_glob`, similarity clears `rule.threshold`, and — the
/// whole point of "no declared relationship" — no edge already connects
/// the two files in either direction.
pub fn semantic_coupling_findings(
    nodes: &[Node],
    edges: &[Edge],
    pairs: &[(NodeId, NodeId, f32)],
    rule: &SemanticCouplingRule,
) -> Vec<SemanticCouplingFinding> {
    let path_of: HashMap<NodeId, &str> = nodes.iter().map(|n| (n.id, n.path.as_str())).collect();
    let declared: HashSet<(&str, &str)> = edges
        .iter()
        .filter_map(|e| {
            let (a, b) = (path_of.get(&e.source_id)?, path_of.get(&e.target_id)?);
            Some(if a <= b { (*a, *b) } else { (*b, *a) })
        })
        .collect();

    let mut findings = Vec::new();
    for &(a, b, similarity) in pairs {
        if similarity < rule.threshold {
            continue;
        }
        let (Some(&file_a), Some(&file_b)) = (path_of.get(&a), path_of.get(&b)) else {
            continue;
        };
        if file_a == file_b {
            continue;
        }
        if !in_module(file_a, &rule.within) || !in_module(file_b, &rule.within) {
            continue;
        }
        if rule
            .exempt_globs
            .iter()
            .any(|g| glob_matches(g, file_a) || glob_matches(g, file_b))
        {
            continue;
        }
        let key = if file_a <= file_b {
            (file_a, file_b)
        } else {
            (file_b, file_a)
        };
        if declared.contains(&key) {
            continue;
        }
        findings.push(SemanticCouplingFinding {
            file_a: key.0.to_string(),
            file_b: key.1.to_string(),
            similarity,
        });
    }
    findings.sort_by(|x, y| {
        (x.file_a.as_str(), x.file_b.as_str()).cmp(&(y.file_a.as_str(), y.file_b.as_str()))
    });
    findings
}

#[cfg(test)]
mod tests;
