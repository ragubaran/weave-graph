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

fn commit_all(dir: &Path, message: &str) {
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
fn pre_push_script_carries_the_marker_the_base_and_both_commands() {
    let script = pre_push_script("main");
    assert!(script.starts_with("#!/bin/sh\n"));
    assert!(script.contains(MARKER));
    assert!(script.contains("weave blast --base main"));
    assert!(script.contains("weave check-contracts --submodules"));
}

#[test]
fn install_writes_an_executable_hook_using_the_detected_default_branch() {
    let dir = init_repo();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(dir.path(), "first");
    let branch = git::current_branch(dir.path()).unwrap();

    cmd_hooks_install(dir.path(), None, false).unwrap();

    let hook_path = git::hooks_dir(dir.path()).unwrap().join("pre-push");
    let content = std::fs::read_to_string(&hook_path).unwrap();
    assert!(content.contains(&format!("weave blast --base {branch}")));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&hook_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "hook must be executable");
    }
}

#[test]
fn install_respects_an_explicit_base_over_the_detected_default() {
    let dir = init_repo();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(dir.path(), "first");

    cmd_hooks_install(dir.path(), Some("develop"), false).unwrap();

    let hook_path = git::hooks_dir(dir.path()).unwrap().join("pre-push");
    let content = std::fs::read_to_string(&hook_path).unwrap();
    assert!(content.contains("weave blast --base develop"));
}

#[test]
fn install_is_idempotent_on_its_own_hook_without_force() {
    let dir = init_repo();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(dir.path(), "first");

    cmd_hooks_install(dir.path(), Some("main"), false).unwrap();
    // Re-running with a different base must succeed (still our own
    // marked hook) and pick up the new base — not refuse as "foreign".
    cmd_hooks_install(dir.path(), Some("develop"), false).unwrap();

    let hook_path = git::hooks_dir(dir.path()).unwrap().join("pre-push");
    let content = std::fs::read_to_string(&hook_path).unwrap();
    assert!(content.contains("weave blast --base develop"));
}

#[test]
fn install_refuses_to_overwrite_a_foreign_pre_push_hook_without_force() {
    let dir = init_repo();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(dir.path(), "first");
    let hooks_dir = git::hooks_dir(dir.path()).unwrap();
    std::fs::write(hooks_dir.join("pre-push"), "#!/bin/sh\necho mine\n").unwrap();

    let err = cmd_hooks_install(dir.path(), Some("main"), false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("wasn't installed by"), "{err}");
    // The foreign hook must survive untouched.
    let content = std::fs::read_to_string(hooks_dir.join("pre-push")).unwrap();
    assert_eq!(content, "#!/bin/sh\necho mine\n");
}

#[test]
fn install_with_force_overwrites_a_foreign_pre_push_hook() {
    let dir = init_repo();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(dir.path(), "first");
    let hooks_dir = git::hooks_dir(dir.path()).unwrap();
    std::fs::write(hooks_dir.join("pre-push"), "#!/bin/sh\necho mine\n").unwrap();

    cmd_hooks_install(dir.path(), Some("main"), true).unwrap();

    let content = std::fs::read_to_string(hooks_dir.join("pre-push")).unwrap();
    assert!(content.contains(MARKER));
}

#[test]
fn install_without_an_explicit_base_or_a_detectable_default_is_a_clear_error() {
    let dir = init_repo();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(dir.path(), "first");
    Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["branch", "-m", "trunk"])
        .status()
        .unwrap();

    let err = cmd_hooks_install(dir.path(), None, false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("--base"), "{err}");
}

#[test]
fn uninstall_removes_a_hook_it_installed() {
    let dir = init_repo();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(dir.path(), "first");
    cmd_hooks_install(dir.path(), Some("main"), false).unwrap();

    cmd_hooks_uninstall(dir.path()).unwrap();

    let hook_path = git::hooks_dir(dir.path()).unwrap().join("pre-push");
    assert!(!hook_path.exists());
}

#[test]
fn uninstall_refuses_to_remove_a_foreign_pre_push_hook() {
    let dir = init_repo();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(dir.path(), "first");
    let hooks_dir = git::hooks_dir(dir.path()).unwrap();
    std::fs::write(hooks_dir.join("pre-push"), "#!/bin/sh\necho mine\n").unwrap();

    let err = cmd_hooks_uninstall(dir.path()).unwrap_err().to_string();
    assert!(err.contains("wasn't installed by"), "{err}");
    assert!(hooks_dir.join("pre-push").exists());
}

#[test]
fn uninstall_with_nothing_installed_is_a_quiet_no_op() {
    let dir = init_repo();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(dir.path(), "first");

    cmd_hooks_uninstall(dir.path()).unwrap();
}
