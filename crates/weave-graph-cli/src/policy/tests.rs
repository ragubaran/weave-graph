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
        BoundaryRule::Disallow(Boundary::new("src/ui", "src/db"))
    );
    assert_eq!(
        rules[1],
        BoundaryRule::Require(Boundary::new("src/service", "src/db"))
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
            extractor: None,
            resolution_kind: None,
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
    let err = cmd_policy_lint(root.path(), None, false, &[], None, false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("1 policy violation"), "{err}");

    write_policy(
        root.path(),
        "rules:\n  - disallow:\n      from: src/db\n      to: src/ui\n",
    );
    cmd_policy_lint(root.path(), None, false, &[], None, false).unwrap();
}

#[test]
fn cmd_lint_requires_an_existing_index() {
    let root = tempfile::tempdir().unwrap();
    write_policy(root.path(), "rules: []\n");
    let err = cmd_policy_lint(root.path(), None, false, &[], None, false)
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

#[cfg(feature = "vector")]
fn indexed_root_with_near_duplicate_files() -> tempfile::TempDir {
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
    let ui_render = symbol("src/ui/view.rs", "render_view");
    let util_render = symbol("src/utils/render_helper.rs", "render_helper");
    let db_save = symbol("src/db/store.rs", "save");
    let embedder = weave_graph_core::embedding::MockEmbeddingProvider::new();
    storage
        .rebuild_vector_index(
            &embedder,
            &[
                (
                    ui_render,
                    "fn check_jwt_ttl(token: &str) -> bool".to_string(),
                ),
                (
                    util_render,
                    "fn check_jwt_ttl(token: &str) -> bool".to_string(),
                ),
                (
                    db_save,
                    "fn render_html_layout(page: &Page) -> String".to_string(),
                ),
            ],
        )
        .unwrap();
    root
}

/// POL-02: two files with near-identical embedded content but no declared
/// edge between them surface as advisory drift, gated purely on
/// `semantic_coupling` being present in `.weave/policy.yaml`.
#[cfg(feature = "vector")]
#[test]
fn drift_flags_undeclared_semantic_coupling_above_the_configured_threshold() {
    let root = indexed_root_with_near_duplicate_files();
    write_policy(
        root.path(),
        "rules: []\nsemantic_coupling:\n  - within: src\n    threshold: 0.9\n",
    );
    let (storage, _db) = crate::open_storage_for_read(root.path()).unwrap();
    let nodes = storage.all_nodes().unwrap();
    let edges = storage.all_edges().unwrap();
    let findings = semantic_coupling_report(root.path(), &storage, &nodes, &edges).unwrap();
    assert_eq!(findings.len(), 1, "{findings:?}");
    let pair = &findings[0];
    assert!(
        (pair.file_a == "src/ui/view.rs" && pair.file_b == "src/utils/render_helper.rs")
            || (pair.file_a == "src/utils/render_helper.rs" && pair.file_b == "src/ui/view.rs"),
        "{findings:?}"
    );

    // The CLI-visible path must not error just because it found something.
    cmd_policy_drift(root.path(), None).unwrap();
}

/// A file pair already connected by a declared edge is never advisory
/// noise, however similar their embedded content — the rule only exists to
/// surface *undeclared* coupling.
#[cfg(feature = "vector")]
#[test]
fn drift_does_not_flag_semantic_coupling_already_covered_by_a_declared_edge() {
    let root = indexed_root_with_near_duplicate_files();
    {
        let weave_dir = root.path().join(".weave");
        let mut storage = SqliteStorage::open(&weave_dir.join("graph.db")).unwrap();
        let nodes = storage.all_nodes().unwrap();
        let ui = nodes
            .iter()
            .find(|n| n.path == "src/ui/view.rs")
            .unwrap()
            .id;
        let util = nodes
            .iter()
            .find(|n| n.path == "src/utils/render_helper.rs")
            .unwrap()
            .id;
        storage
            .upsert_edge(&Edge {
                id: 0,
                source_id: ui,
                target_id: util,
                kind: "CALLS_EXACT".into(),
                weight: 1.0,
                extractor: None,
                resolution_kind: None,
            })
            .unwrap();
    }
    write_policy(
        root.path(),
        "rules: []\nsemantic_coupling:\n  - within: src\n    threshold: 0.9\n",
    );
    let (storage, _db) = crate::open_storage_for_read(root.path()).unwrap();
    let nodes = storage.all_nodes().unwrap();
    let edges = storage.all_edges().unwrap();
    let findings = semantic_coupling_report(root.path(), &storage, &nodes, &edges).unwrap();
    assert!(findings.is_empty(), "{findings:?}");
}

/// No `semantic_coupling` entries in the policy file — the default, since
/// most repos never set it — means zero rules evaluated and an empty
/// report, not an error.
#[cfg(feature = "vector")]
#[test]
fn drift_semantic_coupling_is_empty_without_a_configured_rule() {
    let root = indexed_root_with_near_duplicate_files();
    write_policy(root.path(), "rules: []\n");
    let (storage, _db) = crate::open_storage_for_read(root.path()).unwrap();
    let nodes = storage.all_nodes().unwrap();
    let edges = storage.all_edges().unwrap();
    let findings = semantic_coupling_report(root.path(), &storage, &nodes, &edges).unwrap();
    assert!(findings.is_empty());
}

/// A malformed `threshold` is a config error, not a silently-ignored rule.
#[cfg(feature = "vector")]
#[test]
fn load_semantic_coupling_rules_rejects_an_out_of_range_threshold() {
    let root = tempfile::tempdir().unwrap();
    write_policy(
        root.path(),
        "rules: []\nsemantic_coupling:\n  - within: src\n    threshold: 1.5\n",
    );
    let err = load_semantic_coupling_rules(&root.path().join(POLICY_FILE)).unwrap_err();
    assert!(err.contains("between 0.0 and 1.0"), "{err}");
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
    cmd_policy_lint(root.path(), None, false, &[], None, false).unwrap();
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
    let err = cmd_policy_lint(root.path(), Some("anonymous"), false, &[], None, false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("Policy view incomplete"));

    let explicit_err = cmd_policy_lint(root.path(), Some("anonymous"), true, &[], None, false)
        .unwrap_err()
        .to_string();
    assert!(explicit_err.contains("Policy view incomplete"));
}

/// POL-04: an identity holding a rule's `allowed_roles` is exempted from
/// that specific violation, end to end through `weave policy lint --as`.
#[cfg(feature = "rbac")]
#[test]
fn cmd_lint_exempts_a_violation_for_an_identity_holding_an_allowed_role() {
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
            signature: format!("pub fn {name}()"),
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
            extractor: None,
            resolution_kind: None,
        })
        .unwrap();

    write_policy(
        root.path(),
        "rules:\n  - disallow:\n      from: src/ui\n      to: src/db\n      allowed_roles: [\"data-engineer\"]\n",
    );
    std::fs::write(
        root.path().join(".weave/config.toml"),
        "[rbac.users]\ndata_eng = [\"data-engineer\"]\n",
    )
    .unwrap();

    // No identity: still blocked.
    let err = cmd_policy_lint(root.path(), None, false, &[], None, false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("1 policy violation"), "{err}");

    // Identity holding the exempt role: passes.
    cmd_policy_lint(root.path(), Some("data_eng"), false, &[], None, false).unwrap();
}

/// POL-05, end to end: an authorized `--waive` with `--reason` exempts
/// the named rule from blocking, still prints it, and appends one line
/// to `.weave/policy-waivers.log`.
#[cfg(feature = "rbac")]
#[test]
fn cmd_lint_waive_exempts_a_named_rule_and_logs_it() {
    let root = indexed_root();
    write_policy(
        root.path(),
        "rules:\n  - disallow:\n      from: src/ui\n      to: src/db\n",
    );
    std::fs::write(
        root.path().join(".weave/config.toml"),
        "[rbac.users]\nwaiver_bot = [\"internal\", \"allow-drift\"]\n",
    )
    .unwrap();

    // Unauthorized: no --as, but this repo grants allow-drift to someone
    // (SEC-06), so an anonymous waive attempt is refused.
    let err = cmd_policy_lint(
        root.path(),
        None,
        false,
        &["disallow:src/ui->src/db".to_string()],
        Some("approved incident #42"),
        false,
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("not permitted") || err.contains("not authorized"),
        "{err}"
    );

    // Authorized identity, no --reason: refused with a clear error.
    let err = cmd_policy_lint(
        root.path(),
        Some("waiver_bot"),
        false,
        &["disallow:src/ui->src/db".to_string()],
        None,
        false,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("--reason is required"), "{err}");

    // Authorized identity with a reason: the named rule is waived.
    cmd_policy_lint(
        root.path(),
        Some("waiver_bot"),
        false,
        &["disallow:src/ui->src/db".to_string()],
        Some("approved incident #42"),
        false,
    )
    .unwrap();

    let log = std::fs::read_to_string(root.path().join(".weave/policy-waivers.log")).unwrap();
    assert_eq!(log.lines().count(), 1, "{log}");
    assert!(log.contains("waiver_bot"), "{log}");
    assert!(log.contains("disallow:src/ui->src/db"), "{log}");
    assert!(log.contains("approved incident #42"), "{log}");
}

/// A rule id that doesn't match any current violation is simply a no-op
/// waiver — the real (unrelated) violation still blocks.
#[cfg(feature = "rbac")]
#[test]
fn cmd_lint_waive_of_an_unmatched_rule_id_does_not_exempt_the_real_violation() {
    let root = indexed_root();
    write_policy(
        root.path(),
        "rules:\n  - disallow:\n      from: src/ui\n      to: src/db\n",
    );
    std::fs::write(
        root.path().join(".weave/config.toml"),
        "[rbac.users]\nwaiver_bot = [\"internal\", \"allow-drift\"]\n",
    )
    .unwrap();

    let err = cmd_policy_lint(
        root.path(),
        Some("waiver_bot"),
        false,
        &["disallow:nonexistent->rule".to_string()],
        Some("typo'd rule id"),
        false,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("1 policy violation"), "{err}");
}

/// FED-01, end to end: `--federated` matches `Boundary{from, to}` prefixes
/// across a real `weave link`-built federated graph, using
/// `federation::open_federated_storage`'s own substrate.
#[cfg(feature = "federation")]
#[test]
fn cmd_lint_federated_matches_a_boundary_across_linked_repos() {
    let consumer_dir = tempfile::tempdir().unwrap();
    let provider_dir = tempfile::tempdir().unwrap();
    let consumer_weave = consumer_dir.path().join(".weave");
    let provider_weave = provider_dir.path().join(".weave");
    std::fs::create_dir_all(&consumer_weave).unwrap();
    std::fs::create_dir_all(&provider_weave).unwrap();
    let a_rs = consumer_dir.path().join("a.rs");
    std::fs::write(&a_rs, "fn consumer_fn() { provider_fn(); }\n").unwrap();
    let b_rs = provider_dir.path().join("b.rs");
    std::fs::write(&b_rs, "pub fn provider_fn() {}\n").unwrap();
    crate::index::full_reindex(
        consumer_dir.path(),
        &consumer_weave,
        &consumer_weave.join("graph.db"),
        &[a_rs],
    )
    .unwrap();
    crate::index::full_reindex(
        provider_dir.path(),
        &provider_weave,
        &provider_weave.join("graph.db"),
        std::slice::from_ref(&b_rs),
    )
    .unwrap();
    crate::federation::cmd_link(consumer_dir.path(), provider_dir.path()).unwrap();

    // A rule against the federated graph's own CROSS_REPO edge. `lint`
    // matches purely on each node's own `path` (not `repo_id`) — the same
    // "edge-shape-agnostic" evaluation a single repo's boundaries already
    // use, so a federated rule names the two sides' real relative paths,
    // not a repo-label prefix.
    write_policy(
        consumer_dir.path(),
        "rules:\n  - disallow:\n      from: a.rs\n      to: b.rs\n",
    );
    std::fs::write(
        consumer_dir.path().join(".weave/config.toml"),
        format!(
            "[federation]\nlinked_repos = [\"{}\"]\n",
            provider_dir.path().display()
        ),
    )
    .unwrap();

    let err = cmd_policy_lint(consumer_dir.path(), None, false, &[], None, true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("1 policy violation"), "{err}");
}

#[cfg(feature = "federation")]
#[test]
fn cmd_lint_federated_requires_at_least_one_linked_repo() {
    let root = indexed_root();
    write_policy(root.path(), "rules: []\n");
    let err = cmd_policy_lint(root.path(), None, false, &[], None, true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("No linked repos"), "{err}");
}

#[cfg(not(feature = "federation"))]
#[test]
fn cmd_lint_federated_without_the_feature_is_a_clear_error() {
    let root = indexed_root();
    write_policy(root.path(), "rules: []\n");
    let err = cmd_policy_lint(root.path(), None, false, &[], None, true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("federation"), "{err}");
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
                extractor: None,
                resolution_kind: None,
            })
            .unwrap();
    }
    cmd_policy_drift(root.path(), None).unwrap();
}
