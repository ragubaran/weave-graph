use std::fs;
use std::path::Path;

use super::*;

fn git(root: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn init_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    git(dir.path(), &["config", "user.name", "Test"]);
    dir
}

fn write_source(root: &Path, name: &str, source: &str) -> std::path::PathBuf {
    let path = root.join(format!("{name}.rs"));
    fs::write(&path, source).unwrap();
    path
}

fn index(root: &Path, files: &[std::path::PathBuf]) {
    let weave_dir = root.join(".weave");
    fs::create_dir_all(&weave_dir).unwrap();
    crate::index::full_reindex(root, &weave_dir, &weave_dir.join("graph.db"), files).unwrap();
}

#[test]
fn passes_on_a_clean_repo_with_no_submodules_and_no_phantom_symbols() {
    let dir = init_repo();
    let a = write_source(dir.path(), "a", "pub fn a() {}\nfn b() { a(); }\n");
    index(dir.path(), &[a]);

    let report = cmd_verify(dir.path(), None, None, false).unwrap();
    assert_eq!(report.status, VerifyStatus::Pass);
    assert_eq!(report.status.exit_code(), 0);
    assert!(report.findings.is_empty());
    assert!(report.completed_checks.contains(&"phantom_symbols"));
}

#[test]
fn fails_on_a_phantom_symbol_reference() {
    let dir = init_repo();
    // A call to a symbol that doesn't exist anywhere in the indexed graph.
    let a = write_source(dir.path(), "a", "fn caller() { totally_undefined_fn(); }\n");
    index(dir.path(), &[a]);

    let report = cmd_verify(dir.path(), None, None, false).unwrap();
    assert_eq!(report.status, VerifyStatus::Fail);
    assert_eq!(report.status.exit_code(), 1);
    assert!(
        report.findings.iter().any(|f| f.check == "phantom_symbols"),
        "{:?}",
        report.findings
    );
}

#[test]
fn scoping_to_one_file_limits_the_phantom_symbol_check() {
    let dir = init_repo();
    let a = write_source(dir.path(), "a", "fn caller() { totally_undefined_fn(); }\n");
    let b = write_source(dir.path(), "b", "pub fn clean() {}\n");
    index(dir.path(), &[a, b]);

    let report = cmd_verify(dir.path(), Some("b.rs"), None, false).unwrap();
    assert_eq!(
        report.status,
        VerifyStatus::Pass,
        "b.rs alone has no phantom refs"
    );
    assert_eq!(report.target_file.as_deref(), Some("b.rs"));
}

#[test]
fn range_is_carried_through_to_the_report_without_changing_status() {
    let dir = init_repo();
    let a = write_source(dir.path(), "a", "pub fn a() {}\n");
    index(dir.path(), &[a]);

    let report = cmd_verify(dir.path(), Some("a.rs"), Some((1, 10)), false).unwrap();
    assert_eq!(report.target_range, Some((1, 10)));
}

#[test]
fn is_incomplete_when_a_submodule_is_dirty() {
    let inner = init_repo();
    let inner_a = write_source(inner.path(), "a", "pub fn a() {}\n");
    git(inner.path(), &["add", "-A"]);
    git(inner.path(), &["commit", "-q", "-m", "inner first"]);
    let _ = inner_a;

    let outer = init_repo();
    git(
        outer.path(),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            inner.path().to_str().unwrap(),
            "sub",
        ],
    );
    git(outer.path(), &["add", "-A"]);
    git(outer.path(), &["commit", "-q", "-m", "add submodule"]);
    fs::write(outer.path().join("sub").join("a.rs"), "pub fn a() { 1 }\n").unwrap();
    index(outer.path(), &[outer.path().join("sub").join("a.rs")]);

    let report = cmd_verify(outer.path(), None, None, false).unwrap();
    assert_eq!(report.status, VerifyStatus::Incomplete);
    assert_eq!(report.status.exit_code(), 2);
    assert_eq!(report.submodules.dirty, 1);
}

#[test]
fn fails_on_an_encapsulation_violation_into_a_submodule() {
    let inner = init_repo();
    let inner_a = write_source(inner.path(), "a", "fn private_helper() {}\n");
    git(inner.path(), &["add", "-A"]);
    git(inner.path(), &["commit", "-q", "-m", "inner first"]);
    let _ = inner_a;

    let outer = init_repo();
    git(
        outer.path(),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            inner.path().to_str().unwrap(),
            "sub",
        ],
    );
    let consumer = write_source(
        outer.path(),
        "consumer",
        "fn caller() { private_helper(); }\n",
    );
    git(outer.path(), &["add", "-A"]);
    git(outer.path(), &["commit", "-q", "-m", "add submodule"]);
    index(
        outer.path(),
        &[outer.path().join("sub").join("a.rs"), consumer],
    );

    let report = cmd_verify(outer.path(), None, None, false).unwrap();
    assert_eq!(report.status, VerifyStatus::Fail);
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == "submodule_boundaries"),
        "{:?}",
        report.findings
    );
}

#[test]
fn submodules_only_skips_the_phantom_symbol_check() {
    let dir = init_repo();
    let a = write_source(dir.path(), "a", "fn caller() { totally_undefined_fn(); }\n");
    index(dir.path(), &[a]);

    let report = cmd_verify(dir.path(), None, None, true).unwrap();
    assert_eq!(
        report.status,
        VerifyStatus::Pass,
        "no submodules, no findings expected"
    );
    assert!(
        report
            .skipped_checks
            .iter()
            .any(|(name, _)| *name == "phantom_symbols")
    );
}

#[test]
fn required_guards_is_always_reported_as_skipped() {
    let dir = init_repo();
    let a = write_source(dir.path(), "a", "pub fn a() {}\n");
    index(dir.path(), &[a]);

    let report = cmd_verify(dir.path(), None, None, false).unwrap();
    assert!(
        report
            .skipped_checks
            .iter()
            .any(|(name, _)| *name == "required_guards")
    );
}

fn guard_node(id: weave_graph_core::NodeId, path: &str, symbol: &str) -> Node {
    Node {
        id,
        repo_id: "r".to_string(),
        path: path.to_string(),
        symbol: symbol.to_string(),
        kind: "function".to_string(),
        line_start: 1,
        line_end: 1,
        signature: format!("fn {symbol}()"),
    }
}

fn guard_edge(source_id: weave_graph_core::NodeId, target_id: weave_graph_core::NodeId) -> Edge {
    Edge {
        id: 0,
        source_id,
        target_id,
        kind: "CALLS_EXACT".to_string(),
        weight: 1.0,
        extractor: None,
        resolution_kind: None,
    }
}

#[test]
fn required_guard_violations_flags_a_caller_outside_the_allowlist() {
    let protected = guard_node(1, "admin.rs", "delete_user");
    let bad_caller = guard_node(2, "handlers.rs", "handle_request");
    let nodes = vec![protected.clone(), bad_caller.clone()];
    let edges = vec![guard_edge(2, 1)];
    let guards = vec![RequiredGuardSpec {
        symbol: weave_graph_parse::moniker::build("admin.rs", "delete_user"),
        allowed_callers: vec![weave_graph_parse::moniker::build(
            "rbac.rs",
            "authorize_admin_action",
        )],
    }];

    let findings = required_guard_violations(&nodes, &edges, &guards);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].check, "required_guards");
    assert_eq!(findings[0].file, "handlers.rs");
}

#[test]
fn required_guard_violations_is_empty_when_every_caller_is_allowed() {
    let protected = guard_node(1, "admin.rs", "delete_user");
    let good_caller = guard_node(2, "rbac.rs", "authorize_admin_action");
    let nodes = vec![protected, good_caller];
    let edges = vec![guard_edge(2, 1)];
    let guards = vec![RequiredGuardSpec {
        symbol: weave_graph_parse::moniker::build("admin.rs", "delete_user"),
        allowed_callers: vec![weave_graph_parse::moniker::build(
            "rbac.rs",
            "authorize_admin_action",
        )],
    }];

    assert!(required_guard_violations(&nodes, &edges, &guards).is_empty());
}

#[test]
fn required_guard_violations_is_inert_when_the_declared_symbol_is_not_indexed() {
    let nodes = vec![guard_node(1, "a.rs", "a")];
    let edges = Vec::new();
    let guards = vec![RequiredGuardSpec {
        symbol: weave_graph_parse::moniker::build("nowhere.rs", "ghost"),
        allowed_callers: vec![],
    }];

    assert!(required_guard_violations(&nodes, &edges, &guards).is_empty());
}

#[cfg(feature = "policy-lint")]
#[test]
fn contracts_yml_required_guards_fails_verify_on_a_real_disallowed_caller() {
    let dir = init_repo();
    let admin = write_source(dir.path(), "admin", "pub fn delete_user() {}\n");
    let handlers = write_source(
        dir.path(),
        "handlers",
        "fn handle_request() { delete_user(); }\n",
    );
    index(dir.path(), &[admin, handlers]);
    fs::write(
        dir.path().join(CONTRACTS_FILE),
        format!(
            "required_guards:\n  - symbol: \"{}\"\n    allowed_callers:\n      - \"{}\"\n",
            weave_graph_parse::moniker::build("admin.rs", "delete_user"),
            weave_graph_parse::moniker::build("rbac.rs", "authorize_admin_action"),
        ),
    )
    .unwrap();

    let report = cmd_verify(dir.path(), None, None, false).unwrap();
    assert_eq!(report.status, VerifyStatus::Fail, "{:?}", report.findings);
    assert!(report.completed_checks.contains(&"required_guards"));
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == "required_guards" && f.file == "handlers.rs"),
        "{:?}",
        report.findings
    );
}

#[cfg(feature = "policy-lint")]
#[test]
fn contracts_yml_required_guards_passes_when_the_only_caller_is_allowed() {
    let dir = init_repo();
    let admin = write_source(dir.path(), "admin", "pub fn delete_user() {}\n");
    let rbac = write_source(
        dir.path(),
        "rbac",
        "fn authorize_admin_action() { delete_user(); }\n",
    );
    index(dir.path(), &[admin, rbac]);
    fs::write(
        dir.path().join(CONTRACTS_FILE),
        format!(
            "required_guards:\n  - symbol: \"{}\"\n    allowed_callers:\n      - \"{}\"\n",
            weave_graph_parse::moniker::build("admin.rs", "delete_user"),
            weave_graph_parse::moniker::build("rbac.rs", "authorize_admin_action"),
        ),
    )
    .unwrap();

    let report = cmd_verify(dir.path(), None, None, false).unwrap();
    assert_eq!(report.status, VerifyStatus::Pass, "{:?}", report.findings);
    assert!(report.completed_checks.contains(&"required_guards"));
}

#[test]
fn load_config_defaults_to_every_check_on_when_no_file_exists() {
    let dir = tempfile::tempdir().unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config, VerifyConfig::default());
}

#[test]
fn glob_matches_a_directory_wildcard_but_not_a_sibling() {
    assert!(glob_matches("tests/**", "tests/a.rs"));
    assert!(glob_matches("tests/**", "tests/sub/b.rs"));
    assert!(!glob_matches("tests/**", "src/tests_helper.rs"));
    assert!(glob_matches("a.rs", "a.rs"));
    assert!(!glob_matches("a.rs", "b.rs"));
}

#[cfg(feature = "policy-lint")]
#[test]
fn contracts_yml_exempt_glob_removes_a_phantom_symbol_finding() {
    let dir = init_repo();
    let a = write_source(dir.path(), "a", "fn caller() { totally_undefined_fn(); }\n");
    index(dir.path(), &[a]);
    fs::write(
        dir.path().join(".weave").join("contracts.yml"),
        "ai:\n  phantom_symbols:\n    exempt_globs: [\"a.rs\"]\n",
    )
    .unwrap();

    let report = cmd_verify(dir.path(), None, None, false).unwrap();
    assert_eq!(report.status, VerifyStatus::Pass, "{:?}", report.findings);
}

#[cfg(feature = "policy-lint")]
#[test]
fn contracts_yml_can_disable_the_phantom_symbol_check_entirely() {
    let dir = init_repo();
    let a = write_source(dir.path(), "a", "fn caller() { totally_undefined_fn(); }\n");
    index(dir.path(), &[a]);
    fs::write(
        dir.path().join(".weave").join("contracts.yml"),
        "ai:\n  phantom_symbols:\n    reject_unresolved: false\n",
    )
    .unwrap();

    let report = cmd_verify(dir.path(), None, None, false).unwrap();
    assert_eq!(report.status, VerifyStatus::Pass);
    assert!(
        report
            .skipped_checks
            .iter()
            .any(|(name, _)| *name == "phantom_symbols")
    );
}

#[cfg(feature = "policy-lint")]
#[test]
fn contracts_yml_rejects_malformed_yaml() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".weave")).unwrap();
    fs::write(
        dir.path().join(".weave").join("contracts.yml"),
        "ai: [this is not a mapping\n",
    )
    .unwrap();
    let err = load_config(dir.path()).unwrap_err();
    assert!(err.contains("invalid YAML"), "{err}");
}

#[test]
fn is_incomplete_when_a_submodule_is_uninitialized() {
    let inner = init_repo();
    write_source(inner.path(), "a", "pub fn a() {}\n");
    git(inner.path(), &["add", "-A"]);
    git(inner.path(), &["commit", "-q", "-m", "inner first"]);

    let outer = init_repo();
    git(
        outer.path(),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            inner.path().to_str().unwrap(),
            "sub",
        ],
    );
    git(outer.path(), &["add", "-A"]);
    git(outer.path(), &["commit", "-q", "-m", "add submodule"]);
    git(outer.path(), &["submodule", "deinit", "-q", "-f", "sub"]);
    index(outer.path(), &[]);

    let report = cmd_verify(outer.path(), None, None, false).unwrap();
    assert_eq!(report.status, VerifyStatus::Incomplete);
    assert_eq!(report.status.exit_code(), 2);
}

#[test]
fn encapsulation_check_scopes_findings_to_the_requested_file_and_ignores_unrelated_edges() {
    let inner = init_repo();
    let inner_a = write_source(inner.path(), "a", "fn private_helper() {}\n");
    git(inner.path(), &["add", "-A"]);
    git(inner.path(), &["commit", "-q", "-m", "inner first"]);
    let _ = inner_a;

    let outer = init_repo();
    git(
        outer.path(),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            inner.path().to_str().unwrap(),
            "sub",
        ],
    );
    let consumer = write_source(
        outer.path(),
        "consumer",
        "fn caller() { private_helper(); }\n",
    );
    // An edge entirely outside the submodule — the loop's own boundary
    // filter must skip it, never mistaking it for a violation.
    let other = write_source(
        outer.path(),
        "other",
        "pub fn other_fn() {}\nfn other_caller() { other_fn(); }\n",
    );
    git(outer.path(), &["add", "-A"]);
    git(outer.path(), &["commit", "-q", "-m", "add submodule"]);
    index(
        outer.path(),
        &[
            outer.path().join("sub").join("a.rs"),
            consumer.clone(),
            other,
        ],
    );

    // Scoped to a file that isn't the violator: the finding is hidden.
    let scoped = cmd_verify(outer.path(), Some("other.rs"), None, false).unwrap();
    assert_eq!(scoped.status, VerifyStatus::Pass, "{:?}", scoped.findings);

    // Unscoped: the violation through consumer.rs still surfaces.
    let unscoped = cmd_verify(outer.path(), None, None, false).unwrap();
    assert_eq!(unscoped.status, VerifyStatus::Fail);
    assert!(
        unscoped
            .findings
            .iter()
            .any(|f| f.check == "submodule_boundaries"),
        "{:?}",
        unscoped.findings
    );
}

#[cfg(feature = "policy-lint")]
#[test]
fn contracts_yml_can_disable_submodule_visibility_enforcement() {
    let inner = init_repo();
    let inner_a = write_source(inner.path(), "a", "fn private_helper() {}\n");
    git(inner.path(), &["add", "-A"]);
    git(inner.path(), &["commit", "-q", "-m", "inner first"]);
    let _ = inner_a;

    let outer = init_repo();
    git(
        outer.path(),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            inner.path().to_str().unwrap(),
            "sub",
        ],
    );
    let consumer = write_source(
        outer.path(),
        "consumer",
        "fn caller() { private_helper(); }\n",
    );
    git(outer.path(), &["add", "-A"]);
    git(outer.path(), &["commit", "-q", "-m", "add submodule"]);
    index(
        outer.path(),
        &[outer.path().join("sub").join("a.rs"), consumer],
    );
    // No `ai:` key at all — exercises `AiSection`/`PhantomSymbolsSection`'s
    // own `Default` impls, not just per-field serde defaults.
    fs::write(
        outer.path().join(".weave").join("contracts.yml"),
        "submodules:\n  enforce_visibility: false\n",
    )
    .unwrap();

    let report = cmd_verify(outer.path(), None, None, false).unwrap();
    assert!(
        report
            .skipped_checks
            .iter()
            .any(|(name, _)| *name == "submodule_boundaries"),
        "{:?}",
        report.skipped_checks
    );
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.check == "submodule_boundaries"),
        "visibility enforcement disabled: {:?}",
        report.findings
    );
}

#[test]
fn stale_submodule_references_reports_a_removed_and_a_changed_imported_symbol() {
    let inner = init_repo();
    write_source(inner.path(), "a", "pub fn a() {}\npub fn b() {}\n");
    git(inner.path(), &["add", "-A"]);
    git(inner.path(), &["commit", "-q", "-m", "inner first"]);

    let outer = init_repo();
    git(
        outer.path(),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            inner.path().to_str().unwrap(),
            "sub",
        ],
    );
    let consumer = write_source(outer.path(), "consumer", "fn consumer() { a(); b(); }\n");
    git(outer.path(), &["add", "-A"]);
    git(outer.path(), &["commit", "-q", "-m", "add submodule"]);
    index(
        outer.path(),
        &[outer.path().join("sub").join("a.rs"), consumer],
    );
    // Records the baseline contract before anything drifts.
    let _ = crate::contracts::cmd_check_contracts_submodules(outer.path());

    // `b` is removed and `a`'s signature changes — both called from the
    // parent, so both land in `in_scope` once the pointer bumps.
    write_source(inner.path(), "a", "pub fn a(extra: u32) {}\n");
    git(inner.path(), &["add", "-A"]);
    git(inner.path(), &["commit", "-q", "-m", "inner second"]);
    let inner_sha = String::from_utf8(
        std::process::Command::new("git")
            .arg("-C")
            .arg(inner.path())
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let inner_sha = inner_sha.trim();
    git(
        outer.path().join("sub").as_path(),
        &["fetch", "-q", "origin", inner_sha],
    );
    git(
        outer.path().join("sub").as_path(),
        &["checkout", "-q", inner_sha],
    );

    let report = cmd_verify(outer.path(), None, None, false).unwrap();
    assert_eq!(report.submodules.changed, 1);
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == "stale_submodule_references"),
        "{:?}",
        report.findings
    );
}

#[test]
fn detects_core_and_extended_languages_across_target_files() {
    let dir = init_repo();
    let mut files = vec![
        write_source(dir.path(), "a", "pub fn a() {}\n"),
        {
            let p = dir.path().join("b.py");
            fs::write(&p, "def b():\n    pass\n").unwrap();
            p
        },
        {
            let p = dir.path().join("c.go");
            fs::write(&p, "package main\nfunc C() {}\n").unwrap();
            p
        },
        {
            let p = dir.path().join("d.ts");
            fs::write(&p, "function d(): void {}\n").unwrap();
            p
        },
        {
            let p = dir.path().join("e.js");
            fs::write(&p, "function e() {}\n").unwrap();
            p
        },
        {
            let p = dir.path().join("F.java");
            fs::write(&p, "class F { void f() {} }\n").unwrap();
            p
        },
        {
            let p = dir.path().join("g.c");
            fs::write(&p, "void g(void) {}\n").unwrap();
            p
        },
        {
            let p = dir.path().join("h.cpp");
            fs::write(&p, "void h() {}\n").unwrap();
            p
        },
    ];
    #[cfg(feature = "lang-extended")]
    files.push({
        let p = dir.path().join("i.rb");
        fs::write(&p, "def i\nend\n").unwrap();
        p
    });
    index(dir.path(), &files);

    let report = cmd_verify(dir.path(), None, None, false).unwrap();
    for lang in [
        "rust",
        "python",
        "go",
        "typescript",
        "javascript",
        "java",
        "c",
        "cpp",
    ] {
        assert!(
            report.detected_languages.contains(&lang),
            "missing {lang}: {:?}",
            report.detected_languages
        );
    }
    #[cfg(feature = "lang-extended")]
    assert!(
        report.detected_languages.contains(&"other"),
        "{:?}",
        report.detected_languages
    );
}

#[test]
fn print_text_renders_a_passing_report_with_no_target() {
    let dir = init_repo();
    let a = write_source(dir.path(), "a", "pub fn a() {}\n");
    index(dir.path(), &[a]);

    let report = cmd_verify(dir.path(), None, None, false).unwrap();
    assert_eq!(report.status, VerifyStatus::Pass);
    report.print_text();
}

#[test]
fn print_text_and_json_render_a_failing_report_with_a_target_file_and_range() {
    let dir = init_repo();
    let a = write_source(dir.path(), "a", "fn caller() { totally_undefined_fn(); }\n");
    index(dir.path(), &[a]);

    let report = cmd_verify(dir.path(), Some("a.rs"), Some((1, 5)), false).unwrap();
    assert_eq!(report.status, VerifyStatus::Fail);
    report.print_text();

    let json = report.to_json();
    assert_eq!(json["status"], "fail");
    assert_eq!(json["target"]["file"], "a.rs");
    assert_eq!(json["target"]["range"], serde_json::json!([1, 5]));
    assert!(!json["findings"].as_array().unwrap().is_empty());
}

#[test]
fn print_text_renders_an_incomplete_report_with_submodule_summary() {
    let inner = init_repo();
    let inner_a = write_source(inner.path(), "a", "pub fn a() {}\n");
    git(inner.path(), &["add", "-A"]);
    git(inner.path(), &["commit", "-q", "-m", "inner first"]);
    let _ = inner_a;

    let outer = init_repo();
    git(
        outer.path(),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            inner.path().to_str().unwrap(),
            "sub",
        ],
    );
    git(outer.path(), &["add", "-A"]);
    git(outer.path(), &["commit", "-q", "-m", "add submodule"]);
    fs::write(outer.path().join("sub").join("a.rs"), "pub fn a() { 1 }\n").unwrap();
    index(outer.path(), &[outer.path().join("sub").join("a.rs")]);

    let report = cmd_verify(outer.path(), None, None, false).unwrap();
    assert_eq!(report.status, VerifyStatus::Incomplete);
    assert_eq!(report.submodules.total, 1);
    report.print_text();
}

#[test]
fn json_report_round_trips_through_serde_json() {
    let dir = init_repo();
    let a = write_source(dir.path(), "a", "pub fn a() {}\n");
    index(dir.path(), &[a]);

    let report = cmd_verify(dir.path(), None, None, false).unwrap();
    let json = report.to_json();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["status"], "pass");
    assert!(json["findings"].as_array().unwrap().is_empty());
    assert!(
        !json["coverage"]["completed_checks"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
