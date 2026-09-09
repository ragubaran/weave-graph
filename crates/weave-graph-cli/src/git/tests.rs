use std::fs;
use std::process::Command;

use super::*;

fn init_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    };
    run(&["init", "-q"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test"]);
    dir
}

fn commit_all(dir: &std::path::Path, message: &str) {
    let run = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .unwrap()
    };
    run(&["add", "-A"]);
    run(&["commit", "-q", "-m", message]);
}

#[test]
fn current_sha_returns_a_40_char_hex_commit_id() {
    let dir = init_repo();
    fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(dir.path(), "first");

    let sha = current_sha(dir.path()).unwrap();
    assert_eq!(sha.len(), 40);
    assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn current_sha_is_none_outside_a_git_repo() {
    let dir = tempfile::tempdir().unwrap();
    assert!(current_sha(dir.path()).is_none());
}

#[test]
fn is_working_tree_clean_reflects_uncommitted_changes() {
    let dir = init_repo();
    fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(dir.path(), "first");
    assert!(is_working_tree_clean(dir.path()));

    fs::write(dir.path().join("a.rs"), "fn a() { 1 }\n").unwrap();
    assert!(!is_working_tree_clean(dir.path()));
}

#[test]
fn changed_since_reports_modified_and_untracked_files() {
    let dir = init_repo();
    fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    fs::write(dir.path().join("b.rs"), "fn b() {}\n").unwrap();
    commit_all(dir.path(), "first");
    let sha = current_sha(dir.path()).unwrap();

    fs::write(dir.path().join("a.rs"), "fn a() { 1 }\n").unwrap();
    fs::write(dir.path().join("c.rs"), "fn c() {}\n").unwrap();

    let changed = changed_since(dir.path(), &sha).unwrap();
    assert_eq!(changed, vec!["a.rs".to_string(), "c.rs".to_string()]);
}

#[test]
fn changed_since_is_empty_when_nothing_changed() {
    let dir = init_repo();
    fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(dir.path(), "first");
    let sha = current_sha(dir.path()).unwrap();

    assert!(changed_since(dir.path(), &sha).unwrap().is_empty());
}

#[test]
fn current_branch_returns_the_checked_out_branch_name() {
    let dir = init_repo();
    fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(dir.path(), "first");
    Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["checkout", "-q", "-b", "feature-x"])
        .status()
        .unwrap();

    assert_eq!(current_branch(dir.path()), Some("feature-x".to_string()));
}

#[test]
fn current_branch_is_none_outside_a_git_repo() {
    let dir = tempfile::tempdir().unwrap();
    assert!(current_branch(dir.path()).is_none());
}
