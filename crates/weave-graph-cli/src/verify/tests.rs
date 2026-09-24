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
