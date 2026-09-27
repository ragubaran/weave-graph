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

#[cfg(feature = "federation")]
fn add_submodule(outer: &std::path::Path, inner: &std::path::Path, at: &str) {
    let status = Command::new("git")
        .arg("-C")
        .arg(outer)
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            inner.to_str().unwrap(),
            at,
        ])
        .status()
        .unwrap();
    assert!(status.success(), "git submodule add failed");
}

#[test]
#[cfg(feature = "federation")]
fn discover_submodules_parses_gitmodules() {
    let dir = init_repo();
    fs::write(
        dir.path().join(".gitmodules"),
        "[submodule \"vendor/lib\"]\n\tpath = vendor/lib\n\turl = https://example.test/lib.git\n",
    )
    .unwrap();

    let submodules = discover_submodules(dir.path());
    assert_eq!(submodules.len(), 1);
    assert_eq!(submodules[0].name, "vendor/lib");
    assert_eq!(submodules[0].path, "vendor/lib");
}

#[test]
#[cfg(feature = "federation")]
fn discover_submodules_is_empty_without_a_gitmodules_file() {
    let dir = tempfile::tempdir().unwrap();
    assert!(discover_submodules(dir.path()).is_empty());
}

#[test]
#[cfg(feature = "federation")]
fn submodule_state_is_clean_right_after_a_committed_add() {
    let inner = init_repo();
    fs::write(inner.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(inner.path(), "inner first");

    let outer = init_repo();
    add_submodule(outer.path(), inner.path(), "sub");
    commit_all(outer.path(), "add submodule");

    let submodules = discover_submodules(outer.path());
    assert_eq!(submodules.len(), 1);
    assert_eq!(
        submodule_state(outer.path(), &submodules[0]),
        SubmoduleState::Clean
    );
}

#[test]
#[cfg(feature = "federation")]
fn submodule_state_is_bumped_after_checking_out_a_newer_inner_commit() {
    let inner = init_repo();
    fs::write(inner.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(inner.path(), "inner first");

    let outer = init_repo();
    add_submodule(outer.path(), inner.path(), "sub");
    commit_all(outer.path(), "add submodule");

    fs::write(inner.path().join("a.rs"), "fn a() { 1 }\n").unwrap();
    commit_all(inner.path(), "inner second");
    let sha = current_sha(inner.path()).unwrap();
    Command::new("git")
        .arg("-C")
        .arg(outer.path().join("sub"))
        .args(["fetch", "-q", "origin", &sha])
        .status()
        .unwrap();
    let status = Command::new("git")
        .arg("-C")
        .arg(outer.path().join("sub"))
        .args(["checkout", "-q", &sha])
        .status()
        .unwrap();
    assert!(status.success(), "git checkout of the fetched sha failed");

    let submodules = discover_submodules(outer.path());
    assert_eq!(
        submodule_state(outer.path(), &submodules[0]),
        SubmoduleState::Bumped
    );
}

#[test]
#[cfg(feature = "federation")]
fn submodule_state_is_dirty_with_uncommitted_inner_edits() {
    let inner = init_repo();
    fs::write(inner.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(inner.path(), "inner first");

    let outer = init_repo();
    add_submodule(outer.path(), inner.path(), "sub");
    commit_all(outer.path(), "add submodule");

    fs::write(outer.path().join("sub").join("a.rs"), "fn a() { 2 }\n").unwrap();

    let submodules = discover_submodules(outer.path());
    assert_eq!(
        submodule_state(outer.path(), &submodules[0]),
        SubmoduleState::Dirty
    );
}

#[test]
#[cfg(feature = "federation")]
fn submodule_state_is_uninitialized_before_checkout() {
    let inner = init_repo();
    fs::write(inner.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(inner.path(), "inner first");

    let outer = init_repo();
    add_submodule(outer.path(), inner.path(), "sub");
    commit_all(outer.path(), "add submodule");
    Command::new("git")
        .arg("-C")
        .arg(outer.path())
        .args(["submodule", "deinit", "-q", "-f", "sub"])
        .status()
        .unwrap();

    let submodules = discover_submodules(outer.path());
    assert_eq!(
        submodule_state(outer.path(), &submodules[0]),
        SubmoduleState::Uninitialized
    );
}

#[test]
fn hooks_dir_points_at_the_real_git_hooks_directory() {
    let dir = init_repo();
    let hooks = hooks_dir(dir.path()).unwrap();
    assert!(hooks.is_absolute(), "{hooks:?}");
    // Compare canonicalized paths, not raw ones — `dir.path()` may itself
    // resolve through a symlinked temp directory (e.g. macOS `/tmp` ->
    // `/private/tmp`), which `git`'s own absolute-path output already
    // resolves through.
    let expected = dir.path().join(".git").join("hooks");
    assert_eq!(
        hooks.canonicalize().unwrap_or(hooks),
        expected.canonicalize().unwrap_or(expected)
    );
}

#[test]
fn hooks_dir_is_none_outside_a_git_repo() {
    let dir = tempfile::tempdir().unwrap();
    assert!(hooks_dir(dir.path()).is_none());
}

#[test]
fn default_branch_falls_back_to_a_local_main_or_master_with_no_remote() {
    let dir = init_repo();
    fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(dir.path(), "first");

    // A fresh `git init` may name its initial branch `main` or `master`
    // depending on the local/CI `init.defaultBranch` setting — either is
    // a correct answer here, since there's no `origin` to prefer.
    let branch = default_branch(dir.path()).unwrap();
    assert!(branch == "main" || branch == "master", "{branch}");
    assert_eq!(Some(branch), current_branch(dir.path()));
}

#[test]
fn default_branch_prefers_origin_head_over_a_same_named_local_branch() {
    let upstream = init_repo();
    fs::write(upstream.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(upstream.path(), "first");
    let upstream_branch = current_branch(upstream.path()).unwrap();

    let dir = init_repo();
    fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(dir.path(), "first");
    // Give the local repo its own differently-named branch too, so a
    // correct implementation must be reading `origin/HEAD`, not just
    // "the first of main/master that exists locally".
    Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["branch", "-m", "trunk"])
        .status()
        .unwrap();
    Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["remote", "add", "origin", upstream.path().to_str().unwrap()])
        .status()
        .unwrap();
    Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["fetch", "-q", "origin"])
        .status()
        .unwrap();
    Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["remote", "set-head", "origin", &upstream_branch])
        .status()
        .unwrap();

    assert_eq!(default_branch(dir.path()), Some(upstream_branch));
}

#[test]
fn default_branch_is_none_with_no_remote_and_no_main_or_master() {
    let dir = init_repo();
    fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(dir.path(), "first");
    Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["branch", "-m", "trunk"])
        .status()
        .unwrap();

    assert_eq!(default_branch(dir.path()), None);
}
