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
        BoundaryRule::Disallow(Boundary {
            from: "src/ui".to_string(),
            to: "src/db".to_string(),
        }),
        BoundaryRule::Disallow(Boundary {
            from: "src/utils".to_string(),
            to: "src/db".to_string(),
        }),
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
fn require_flags_missing_edges() {
    let nodes = nodes();
    // service -> db crosses exist; service -> utils does not.
    let edges = vec![edge(1, 3, 2)];
    let rules = vec![
        BoundaryRule::Require(Boundary {
            from: "src/service".to_string(),
            to: "src/db".to_string(),
        }),
        BoundaryRule::Require(Boundary {
            from: "src/service".to_string(),
            to: "src/utils".to_string(),
        }),
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
        BoundaryRule::Require(Boundary {
            from: "z".to_string(),
            to: "b".to_string(),
        }),
        BoundaryRule::Disallow(Boundary {
            from: "src/db".to_string(),
            to: "src/ui".to_string(),
        }),
        BoundaryRule::Disallow(Boundary {
            from: "src/ui".to_string(),
            to: "src/db".to_string(),
        }),
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
