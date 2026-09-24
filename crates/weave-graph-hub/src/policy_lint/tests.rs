use weave_graph_core::{Edge, Node, Storage};
use weave_graph_store_sqlite::SqliteStorage;

use super::*;

fn node(id: u32, path: &str, symbol: &str) -> Node {
    Node {
        id,
        repo_id: "r".to_string(),
        path: path.to_string(),
        symbol: symbol.to_string(),
        kind: "function".to_string(),
        line_start: 1,
        line_end: 2,
        signature: symbol.to_string(),
    }
}

fn snapshot_bytes(nodes: &[Node], edges: &[Edge]) -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("graph.db");
    {
        let mut storage = SqliteStorage::open(&db_path).unwrap();
        for n in nodes {
            storage.upsert_node(n).unwrap();
        }
        for e in edges {
            storage.upsert_edge(e).unwrap();
        }
    }
    std::fs::read(&db_path).unwrap()
}

#[test]
fn load_rules_is_empty_when_no_policy_file_exists() {
    let dir = tempfile::tempdir().unwrap();
    let rules = load_rules(dir.path()).unwrap();
    assert!(rules.is_empty());
}

#[test]
fn load_rules_parses_the_registry_policy_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("policy.yaml"),
        "rules:\n  - disallow:\n      from: src/ui\n      to: src/db\n",
    )
    .unwrap();
    let rules = load_rules(dir.path()).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(
        rules[0],
        BoundaryRule::Disallow(Boundary::new("src/ui", "src/db"))
    );
}

#[test]
fn load_rules_rejects_a_malformed_rule() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("policy.yaml"), "rules:\n  - from: a\n").unwrap();
    let err = load_rules(dir.path()).unwrap_err();
    assert!(err.contains("exactly one of"), "{err}");
}

#[test]
fn lint_snapshot_bytes_flags_a_disallowed_crossing() {
    let bytes = snapshot_bytes(
        &[
            node(1, "src/ui/view.rs", "render"),
            node(2, "src/db/store.rs", "save"),
        ],
        &[Edge {
            id: 0,
            source_id: 1,
            target_id: 2,
            kind: "CALLS_EXACT".to_string(),
            weight: 1.0,
            extractor: None,
            resolution_kind: None,
        }],
    );
    let rules = vec![BoundaryRule::Disallow(Boundary::new("src/ui", "src/db"))];
    let violations = lint_snapshot_bytes(&bytes, &rules).unwrap();
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].kind, "disallow");
}

#[test]
fn lint_snapshot_bytes_is_clean_for_a_compliant_graph() {
    let bytes = snapshot_bytes(
        &[
            node(1, "src/ui/view.rs", "render"),
            node(2, "src/utils/text.rs", "trim"),
        ],
        &[Edge {
            id: 0,
            source_id: 1,
            target_id: 2,
            kind: "CALLS_EXACT".to_string(),
            weight: 1.0,
            extractor: None,
            resolution_kind: None,
        }],
    );
    let rules = vec![BoundaryRule::Disallow(Boundary::new("src/ui", "src/db"))];
    let violations = lint_snapshot_bytes(&bytes, &rules).unwrap();
    assert!(violations.is_empty());
}
