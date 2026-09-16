use super::*;

fn write_policy(root: &Path, yaml: &str) {
    let path = root.join(POLICY_FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, yaml).unwrap();
}

#[test]
fn load_rules_parses_both_actions() {
    let root = tempfile::tempdir().unwrap();
    write_policy(
        root.path(),
        "rules:\n  - disallow:\n      from: src/ui\n      to: src/db\n  - require:\n      from: src/service\n      to: src/db\n",
    );
    let rules = load_rules(&root.path().join(POLICY_FILE)).unwrap();
    assert_eq!(rules.len(), 2);
    assert_eq!(
        rules[0],
        BoundaryRule::Disallow(Boundary {
            from: "src/ui".into(),
            to: "src/db".into()
        })
    );
    assert_eq!(
        rules[1],
        BoundaryRule::Require(Boundary {
            from: "src/service".into(),
            to: "src/db".into()
        })
    );
}

#[test]
fn load_rules_rejects_config_errors_loudly() {
    let root = tempfile::tempdir().unwrap();
    let cases = [
        (
            "rules:\n  - disallow:\n      from: a\n      to: b\n    require:\n      from: c\n      to: d\n",
            "cannot be both",
        ),
        (
            "rules:\n  - from: a\n",
            "exactly one of `disallow` or `require`",
        ),
        (
            "rules:\n  - disallow:\n      from: \"\"\n      to: b\n",
            "non-empty path prefixes",
        ),
    ];
    for (yaml, case) in cases {
        write_policy(root.path(), yaml);
        let err = load_rules(&root.path().join(POLICY_FILE)).unwrap_err();
        assert!(
            err.contains(case) || err.contains("invalid policy YAML"),
            "case {case}: got {err:?}"
        );
    }
}

#[test]
fn load_rules_reports_a_missing_file_as_config_guidance() {
    let root = tempfile::tempdir().unwrap();
    let err = load_rules(&root.path().join(POLICY_FILE)).unwrap_err();
    assert!(err.contains("policy file not found"));
    assert!(err.contains("declare boundaries"));
}

#[test]
fn empty_rules_file_lints_clean() {
    let root = tempfile::tempdir().unwrap();
    write_policy(root.path(), "rules: []\n");
    let rules = load_rules(&root.path().join(POLICY_FILE)).unwrap();
    assert!(rules.is_empty());
}

use weave_graph_core::{Edge, Storage};
use weave_graph_store_sqlite::SqliteStorage;

fn indexed_root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let weave_dir = root.path().join(".weave");
    std::fs::create_dir_all(&weave_dir).unwrap();
    let mut storage = SqliteStorage::open(&weave_dir.join("graph.db")).unwrap();
    let mut symbol = |path: &str, name: &str| {
        let node = weave_graph_core::Node {
            id: 0,
            repo_id: "local".into(),
            path: path.into(),
            symbol: name.into(),
            kind: "function".into(),
            line_start: 1,
            line_end: 2,
            signature: format!("fn {name}()"),
        };
        storage.upsert_node(&node).unwrap()
    };
    let render = symbol("src/ui/view.rs", "render");
    let save = symbol("src/db/store.rs", "save");
    storage
        .upsert_edge(&Edge {
            id: 0,
            source_id: render,
            target_id: save,
            kind: "CALLS_EXACT".into(),
            weight: 1.0,
        })
        .unwrap();
    root
}
/// The CI gate itself, at the function level: the boundary the fixture
/// repo violates fails the command; one it respects passes.
#[test]
fn cmd_lint_blocks_violation_and_passes_compliance() {
    let root = indexed_root();
    write_policy(
        root.path(),
        "rules:\n  - disallow:\n      from: src/ui\n      to: src/db\n",
    );
    let err = cmd_policy_lint(root.path(), None, false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("1 policy violation"), "{err}");

    write_policy(
        root.path(),
        "rules:\n  - disallow:\n      from: src/db\n      to: src/ui\n",
    );
    cmd_policy_lint(root.path(), None, false).unwrap();
}

#[test]
fn cmd_lint_requires_an_existing_index() {
    let root = tempfile::tempdir().unwrap();
    write_policy(root.path(), "rules: []\n");
    let err = cmd_policy_lint(root.path(), None, false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("No graph database found"), "{err}");
}

#[test]
fn cmd_drift_reports_without_blocking() {
    let root = indexed_root();
    write_policy(root.path(), "rules: []\n");
    cmd_policy_drift(root.path(), None).unwrap();
}

/// Confirmed ADR obligations print as
/// advisory context and never block.
#[cfg(feature = "slm")]
#[test]
fn cmd_lint_surfaces_confirmed_adr_obligations_advisory() {
    let root = indexed_root();
    write_policy(root.path(), "rules: []\n");
    std::fs::write(
        root.path().join(".weave/rules.toml"),
        "[[confirmed]]\nfile = \"docs/adr.md\"\nline = 3\ntext = \"handlers must validate request ids\"\n",
    )
    .unwrap();
    cmd_policy_lint(root.path(), None, false).unwrap();
}

/// A linted view under an rbac-masked identity skips
/// edges whose endpoints it cannot classify — counted and reported,
/// never guessed into violations.
#[cfg(feature = "rbac")]
#[test]
fn cmd_lint_skips_rbac_masked_edges_and_says_so() {
    let root = indexed_root();
    write_policy(
        root.path(),
        "rules:\n  - disallow:\n      from: src/ui\n      to: src/db\n",
    );
    std::fs::write(
        root.path().join(".weave/config.toml"),
        "[rbac.users]\nanonymous = []\n",
    )
    .unwrap();
    // `render`/`save` are Rust `fn`s with no `pub` — anonymous sees
    // nothing, so the ui->db edge is unclassifiable and must not become
    // a violation.
    let err = cmd_policy_lint(root.path(), Some("anonymous"), false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("Policy view incomplete"));

    let explicit_err = cmd_policy_lint(root.path(), Some("anonymous"), true)
        .unwrap_err()
        .to_string();
    assert!(explicit_err.contains("Policy view incomplete"));
}

/// A two-file ring gives drift both remaining branches: a cycle *and*
/// zero orphans (both files have inbound edges).
#[test]
fn cmd_drift_reports_a_cycle_and_no_orphans() {
    let root = tempfile::tempdir().unwrap();
    let weave_dir = root.path().join(".weave");
    std::fs::create_dir_all(&weave_dir).unwrap();
    let mut storage = SqliteStorage::open(&weave_dir.join("graph.db")).unwrap();
    let mut symbol = |path: &str, name: &str| {
        let node = weave_graph_core::Node {
            id: 0,
            repo_id: "local".into(),
            path: path.into(),
            symbol: name.into(),
            kind: "function".into(),
            line_start: 1,
            line_end: 2,
            signature: format!("fn {name}()"),
        };
        storage.upsert_node(&node).unwrap()
    };
    let ping = symbol("ping.rs", "ping");
    let pong = symbol("pong.rs", "pong");
    for (a, b) in [(ping, pong), (pong, ping)] {
        storage
            .upsert_edge(&Edge {
                id: 0,
                source_id: a,
                target_id: b,
                kind: "CALLS_EXACT".into(),
                weight: 1.0,
            })
            .unwrap();
    }
    cmd_policy_drift(root.path(), None).unwrap();
}
