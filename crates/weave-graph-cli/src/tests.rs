use std::fs;

use super::*;

#[test]
fn ensure_gitignored_creates_gitignore_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    ensure_gitignored(dir.path()).unwrap();
    let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(content.lines().any(|l| l == ".weave/"));
}

#[test]
fn ensure_gitignored_appends_to_an_existing_gitignore() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
    ensure_gitignored(dir.path()).unwrap();
    let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(content.lines().any(|l| l == "target/"));
    assert!(content.lines().any(|l| l == ".weave/"));
}

#[test]
fn ensure_gitignored_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    ensure_gitignored(dir.path()).unwrap();
    ensure_gitignored(dir.path()).unwrap();
    let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert_eq!(content.lines().filter(|l| l.contains(".weave")).count(), 1);
}

#[test]
fn ensure_gitignored_respects_an_existing_weave_entry() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), ".weave\n").unwrap();
    ensure_gitignored(dir.path()).unwrap();
    let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert_eq!(content, ".weave\n");
}

#[test]
fn should_skip_dir_excludes_known_noise_directories() {
    assert!(should_skip_dir(".git"));
    assert!(should_skip_dir("node_modules"));
    assert!(should_skip_dir(".weave"));
    assert!(!should_skip_dir("src"));
}
