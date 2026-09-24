use super::*;

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

#[test]
fn reports_a_disallowed_boundary_crossing() {
    let dir = tempfile::tempdir().unwrap();
    let policy_path = dir.path().join("policy.yaml");
    std::fs::write(
        &policy_path,
        "rules:\n  - disallow:\n      from: src/ui\n      to: src/db\n",
    )
    .unwrap();
    let nodes = vec![
        node(1, "src/ui/view.rs", "render"),
        node(2, "src/db/store.rs", "save"),
    ];
    let edges = vec![edge(1, 1, 2)];

    let text = weave_policy_lint(&policy_path, &nodes, &edges, &[]).unwrap();

    assert!(text.contains("1 policy violation"), "{text}");
    assert!(text.contains("src/ui -> src/db"), "{text}");
}

#[test]
fn reports_no_violations_for_a_compliant_graph() {
    let dir = tempfile::tempdir().unwrap();
    let policy_path = dir.path().join("policy.yaml");
    std::fs::write(
        &policy_path,
        "rules:\n  - disallow:\n      from: src/ui\n      to: src/db\n",
    )
    .unwrap();
    let nodes = vec![
        node(1, "src/ui/view.rs", "render"),
        node(2, "src/utils/text.rs", "trim"),
    ];
    let edges = vec![edge(1, 1, 2)];

    let text = weave_policy_lint(&policy_path, &nodes, &edges, &[]).unwrap();

    assert_eq!(text, "✓ no boundary violations");
}

#[test]
fn an_allowed_role_exempts_the_violation_and_owner_role_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let policy_path = dir.path().join("policy.yaml");
    std::fs::write(
        &policy_path,
        "rules:\n  - disallow:\n      from: src/ui\n      to: src/db\n      allowed_roles: [\"data-engineer\"]\n      owner_role: \"platform-team\"\n",
    )
    .unwrap();
    let nodes = vec![
        node(1, "src/ui/view.rs", "render"),
        node(2, "src/db/store.rs", "save"),
    ];
    let edges = vec![edge(1, 1, 2)];

    let unscoped = weave_policy_lint(&policy_path, &nodes, &edges, &[]).unwrap();
    assert!(unscoped.contains("owner: platform-team"), "{unscoped}");

    let scoped =
        weave_policy_lint(&policy_path, &nodes, &edges, &["data-engineer".to_string()]).unwrap();
    assert_eq!(scoped, "✓ no boundary violations");
}

#[test]
fn a_missing_policy_file_is_a_reported_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nope.yaml");

    let err = weave_policy_lint(&missing, &[], &[], &[]).unwrap_err();

    assert!(err.contains("policy file not found"), "{err}");
}
