use super::*;
use crate::model::{Edge, Node};

fn node(id: u32, path: &str, symbol: &str) -> Node {
    Node {
        id,
        repo_id: "local".to_string(),
        path: path.to_string(),
        symbol: symbol.to_string(),
        kind: "function".to_string(),
        line_start: 1,
        line_end: 2,
        signature: symbol.to_string(),
    }
}

fn edge(id: u32, source: u32, target: u32) -> Edge {
    Edge {
        id,
        source_id: source,
        target_id: target,
        kind: "CALLS_EXACT".to_string(),
        weight: 1.0,
        extractor: None,
        resolution_kind: None,
    }
}

fn nodes() -> Vec<Node> {
    vec![
        node(1, "src/ui/view.rs", "render"),
        node(2, "src/db/store.rs", "save"),
        node(3, "src/service/api.rs", "handle"),
        node(4, "src/utils/text.rs", "trim"),
    ]
}

#[test]
fn module_prefix_is_boundary_safe() {
    assert!(in_module("src/ui/view.rs", "src/ui"));
    assert!(in_module("src/ui", "src/ui"));
    assert!(!in_module("src/utils/text.rs", "src/ui"));
    assert!(in_module("anything.rs", ""));
}

#[test]
fn disallow_flags_crossing_edges_and_keeps_compliant_graphs_clean() {
    let nodes = nodes();
    let edges = vec![edge(1, 1, 2), edge(2, 1, 4)];
    let rules = vec![
        BoundaryRule::Disallow(Boundary::new("src/ui".to_string(), "src/db".to_string())),
        BoundaryRule::Disallow(Boundary::new("src/utils".to_string(), "src/db".to_string())),
    ];
    let violations = lint(&nodes, &edges, &rules);
    assert_eq!(violations.len(), 1);
    let v = &violations[0];
    assert_eq!(v.kind, "disallow");
    assert_eq!(v.from, "src/ui");
    assert_eq!(v.to, "src/db");
    assert_eq!(
        v.examples,
        vec!["render (src/ui/view.rs) -> save (src/db/store.rs)"]
    );

    assert!(lint(&nodes, &[edge(1, 1, 4)], &rules).is_empty());
}

#[test]
fn lint_scoped_exempts_a_violation_when_a_role_matches_allowed_roles() {
    let nodes = nodes();
    let edges = vec![edge(1, 1, 2)];
    let mut boundary = Boundary::new("src/ui", "src/db");
    boundary.allowed_roles = vec!["data-engineer".to_string()];
    let rules = vec![BoundaryRule::Disallow(boundary)];

    // No matching role: still a violation.
    assert_eq!(
        lint_scoped(&nodes, &edges, &rules, &["other-role".to_string()]).len(),
        1
    );
    // Matching role: exempted.
    assert!(lint_scoped(&nodes, &edges, &rules, &["data-engineer".to_string()]).is_empty());
    // `lint()` itself never exempts anything — no roles applied.
    assert_eq!(lint(&nodes, &edges, &rules).len(), 1);
}

#[test]
fn lint_scoped_carries_owner_role_onto_the_violation_without_gating_it() {
    let nodes = nodes();
    let edges = vec![edge(1, 1, 2)];
    let mut boundary = Boundary::new("src/ui", "src/db");
    boundary.owner_role = Some("platform-team".to_string());
    let rules = vec![BoundaryRule::Disallow(boundary)];

    let violations = lint_scoped(&nodes, &edges, &rules, &[]);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].owner_role.as_deref(), Some("platform-team"));
}

#[test]
fn require_flags_missing_edges() {
    let nodes = nodes();
    // service -> db crosses exist; service -> utils does not.
    let edges = vec![edge(1, 3, 2)];
    let rules = vec![
        BoundaryRule::Require(Boundary::new(
            "src/service".to_string(),
            "src/db".to_string(),
        )),
        BoundaryRule::Require(Boundary::new(
            "src/service".to_string(),
            "src/utils".to_string(),
        )),
    ];
    let violations = lint(&nodes, &edges, &rules);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].kind, "require");
    assert_eq!(violations[0].to, "src/utils");
}

#[test]
fn violations_are_sorted_deterministically() {
    let nodes = nodes();
    let edges = vec![edge(1, 1, 2), edge(2, 2, 1)];
    let rules = vec![
        BoundaryRule::Require(Boundary::new("z".to_string(), "b".to_string())),
        BoundaryRule::Disallow(Boundary::new("src/db".to_string(), "src/ui".to_string())),
        BoundaryRule::Disallow(Boundary::new("src/ui".to_string(), "src/db".to_string())),
    ];
    let violations = lint(&nodes, &edges, &rules);
    let keys: Vec<(&str, &str, &str)> = violations
        .iter()
        .map(|v| (v.kind, v.from.as_str(), v.to.as_str()))
        .collect();
    assert_eq!(
        keys,
        vec![
            ("disallow", "src/db", "src/ui"),
            ("disallow", "src/ui", "src/db"),
            ("require", "z", "b"),
        ]
    );
}

#[test]
fn find_cycles_reports_each_ring_once() {
    let nodes = vec![
        node(1, "a.rs", "a"),
        node(2, "b.rs", "b"),
        node(3, "c.rs", "c"),
        node(4, "d.rs", "d"),
    ];
    // a -> b -> c -> a is a ring; b -> d is a tail; self-edges ignored.
    let edges = vec![
        edge(1, 1, 2),
        edge(2, 2, 3),
        edge(3, 3, 1),
        edge(4, 2, 4),
        edge(5, 1, 1),
    ];
    let cycles = find_cycles(&nodes, &edges);
    assert_eq!(cycles.len(), 1);
    assert_eq!(cycles[0], vec!["a.rs", "b.rs", "c.rs", "a.rs"]);
}

#[test]
fn acyclic_graph_yields_no_cycles() {
    let nodes = nodes();
    let edges = vec![edge(1, 1, 2), edge(2, 2, 3), edge(3, 4, 1)];
    assert!(find_cycles(&nodes, &edges).is_empty());
}

#[test]
fn orphan_files_have_no_inbound_cross_file_edges() {
    let nodes = vec![
        node(1, "a.rs", "a"),
        node(2, "b.rs", "b"),
        node(3, "c.rs", "c"),
    ];
    let edges = vec![edge(1, 1, 2), edge(2, 2, 2)];
    // b is targeted (by a); c has no edges at all; a's only edge is
    // outbound. Same-file self-edge (b -> b) doesn't count as inbound.
    assert_eq!(orphan_files(&nodes, &edges), vec!["a.rs", "c.rs"]);
}

fn coupling_rule(within: &str, threshold: f32, exempt_globs: &[&str]) -> SemanticCouplingRule {
    SemanticCouplingRule {
        within: within.to_string(),
        threshold,
        exempt_globs: exempt_globs.iter().map(|g| g.to_string()).collect(),
    }
}

#[test]
fn semantic_coupling_flags_a_high_similarity_pair_with_no_declared_edge() {
    let nodes = nodes();
    let edges = Vec::new();
    let pairs = vec![(1u32, 4u32, 0.95f32)];
    let rule = coupling_rule("src", 0.9, &[]);
    let findings = semantic_coupling_findings(&nodes, &edges, &pairs, &rule);
    assert_eq!(
        findings,
        vec![SemanticCouplingFinding {
            file_a: "src/ui/view.rs".to_string(),
            file_b: "src/utils/text.rs".to_string(),
            similarity: 0.95,
        }]
    );
}

#[test]
fn semantic_coupling_ignores_a_pair_already_connected_by_a_declared_edge() {
    let nodes = nodes();
    let edges = vec![edge(1, 1, 4)];
    let pairs = vec![(1u32, 4u32, 0.95f32)];
    let rule = coupling_rule("src", 0.9, &[]);
    assert!(semantic_coupling_findings(&nodes, &edges, &pairs, &rule).is_empty());
}

#[test]
fn semantic_coupling_ignores_a_pair_below_threshold() {
    let nodes = nodes();
    let pairs = vec![(1u32, 4u32, 0.5f32)];
    let rule = coupling_rule("src", 0.9, &[]);
    assert!(semantic_coupling_findings(&nodes, &Vec::new(), &pairs, &rule).is_empty());
}

#[test]
fn semantic_coupling_ignores_a_pair_outside_the_within_scope() {
    // node 4 (src/utils/text.rs) falls outside a "src/ui" scope.
    let nodes = nodes();
    let pairs = vec![(1u32, 4u32, 0.95f32)];
    let rule = coupling_rule("src/ui", 0.9, &[]);
    assert!(semantic_coupling_findings(&nodes, &Vec::new(), &pairs, &rule).is_empty());
}

#[test]
fn semantic_coupling_exempt_glob_drops_a_matching_file_on_either_side() {
    let nodes = nodes();
    let pairs = vec![(1u32, 4u32, 0.95f32)];
    let rule = coupling_rule("src", 0.9, &["src/utils/**"]);
    assert!(semantic_coupling_findings(&nodes, &Vec::new(), &pairs, &rule).is_empty());
}

#[test]
fn semantic_coupling_dedupes_the_declared_edge_check_regardless_of_direction() {
    let nodes = nodes();
    // Declared edge runs 4 -> 1; the similarity pair is reported as (1, 4).
    // Both must resolve to the same undirected file pair.
    let edges = vec![edge(1, 4, 1)];
    let pairs = vec![(1u32, 4u32, 0.95f32)];
    let rule = coupling_rule("src", 0.9, &[]);
    assert!(semantic_coupling_findings(&nodes, &edges, &pairs, &rule).is_empty());
}
