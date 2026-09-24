use std::fs;
use std::process::Command;

use super::*;

fn git_init(root: &std::path::Path) {
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    };
    run(&["init", "-q"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test"]);
}

fn git_commit_all(root: &std::path::Path, message: &str) {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["add", "-A"])
        .status()
        .unwrap();
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["commit", "-q", "-m", message])
        .status()
        .unwrap();
}

fn head_sha(root: &std::path::Path) -> String {
    git(root, &["rev-parse", "HEAD"]).unwrap()
}

#[test]
fn is_unknown_outside_a_git_repo() {
    let dir = tempfile::tempdir().unwrap();
    let weave_dir = dir.path().join(".weave");
    fs::create_dir_all(&weave_dir).unwrap();
    assert_eq!(check_freshness(&weave_dir), Freshness::Unknown);
}

#[test]
fn is_behind_with_no_last_indexed_sha_recorded() {
    let dir = tempfile::tempdir().unwrap();
    git_init(dir.path());
    fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    git_commit_all(dir.path(), "first");
    let weave_dir = dir.path().join(".weave");
    fs::create_dir_all(&weave_dir).unwrap();

    let freshness = check_freshness(&weave_dir);
    assert_eq!(
        freshness,
        Freshness::Behind {
            indexed_sha: None,
            head_sha: head_sha(dir.path()),
        }
    );
}

#[test]
fn is_behind_when_the_indexed_sha_predates_head() {
    let dir = tempfile::tempdir().unwrap();
    git_init(dir.path());
    fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    git_commit_all(dir.path(), "first");
    let weave_dir = dir.path().join(".weave");
    fs::create_dir_all(&weave_dir).unwrap();
    fs::write(
        weave_dir.join("last_indexed_sha"),
        "0000000000000000000000000000000000000000",
    )
    .unwrap();

    let freshness = check_freshness(&weave_dir);
    assert_eq!(
        freshness,
        Freshness::Behind {
            indexed_sha: Some("0000000000000000000000000000000000000000".to_string()),
            head_sha: head_sha(dir.path()),
        }
    );
}

#[test]
fn is_fresh_when_indexed_sha_matches_head_and_tree_is_clean() {
    let dir = tempfile::tempdir().unwrap();
    git_init(dir.path());
    fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    git_commit_all(dir.path(), "first");
    let weave_dir = dir.path().join(".weave");
    fs::create_dir_all(&weave_dir).unwrap();
    fs::write(weave_dir.join("last_indexed_sha"), head_sha(dir.path())).unwrap();

    assert_eq!(check_freshness(&weave_dir), Freshness::Fresh);
}

#[test]
fn is_dirty_when_indexed_sha_matches_head_but_tree_has_uncommitted_edits() {
    let dir = tempfile::tempdir().unwrap();
    git_init(dir.path());
    fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    git_commit_all(dir.path(), "first");
    let weave_dir = dir.path().join(".weave");
    fs::create_dir_all(&weave_dir).unwrap();
    fs::write(weave_dir.join("last_indexed_sha"), head_sha(dir.path())).unwrap();

    fs::write(dir.path().join("a.rs"), "fn a() { 1 }\n").unwrap();

    assert_eq!(check_freshness(&weave_dir), Freshness::Dirty);
}
