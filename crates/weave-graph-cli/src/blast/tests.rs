use std::fs;
use std::path::Path;
use std::process::Command;

use super::*;
use crate::git::blast_since;

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn init_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["config", "user.email", "[EMAIL]"]);
    git(dir.path(), &["config", "user.name", "Test"]);
    dir
}

fn commit_all(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
}

/// M2.12's fixture shape: a 3-commit PR branch with a `main` that moved
/// after the branch point.
struct PrFixture {
    dir: tempfile::TempDir,
    weave_dir: std::path::PathBuf,
    active_db: std::path::PathBuf,
}

impl PrFixture {
    fn new() -> Self {
        let dir = init_repo();
        // Base: a shared helper plus the PR file, committed on main.
        fs::write(dir.path().join("core.rs"), "fn core() {}\n").unwrap();
        fs::write(
            dir.path().join("feature.rs"),
            "fn feature() { core(); }\nfn feature_caller() { feature(); }\n",
        )
        .unwrap();
        commit_all(dir.path(), "base");
        // PR branch: three commits, each touching only feature.rs.
        git(dir.path(), &["checkout", "-q", "-b", "pr"]);
        for i in 1..=3 {
            let src = fs::read_to_string(dir.path().join("feature.rs")).unwrap();
            fs::write(
                dir.path().join("feature.rs"),
                format!("{src}// pr change {i}\n"),
            )
            .unwrap();
            commit_all(dir.path(), &format!("pr {i}"));
        }
        // main moves after the branch point, touching a different file.
        git(dir.path(), &["checkout", "-q", "main"]);
        fs::write(dir.path().join("unrelated.rs"), "fn unrelated() {}\n").unwrap();
        commit_all(dir.path(), "main moves on");
        git(dir.path(), &["checkout", "-q", "pr"]);

        let weave_dir = dir.path().join(".weave");
        fs::create_dir_all(&weave_dir).unwrap();
        let active_db = weave_dir.join("graph.db");
        Self {
            dir,
            weave_dir,
            active_db,
        }
    }
}

#[test]
fn blast_since_reports_only_pr_side_changes_with_a_moved_main() {
    let fx = PrFixture::new();
    let changed = blast_since(fx.dir.path(), "main").unwrap();
    assert_eq!(changed, vec!["feature.rs".to_string()]);
    assert!(
        !changed.iter().any(|p| p.contains("unrelated")),
        "a moved main must never be blamed on the PR (three-dot diff)"
    );
}

#[test]
fn blast_since_refuses_a_shallow_checkout_with_a_clear_message() {
    let dir = init_repo();
    fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(dir.path(), "first");
    // `git rev-parse --is-shallow-repository` reports shallow purely on
    // this file's existence — the same signal a real fetch-depth:1 clone
    // produces.
    fs::write(dir.path().join(".git").join("shallow"), "").unwrap();

    let err = blast_since(dir.path(), "main").unwrap_err();
    assert!(err.contains("fetch-depth"), "must name the fix: {err}");
    assert!(err.contains("shallow"), "{err}");
}

#[test]
fn blast_since_names_an_unknown_ref_clearly() {
    let dir = init_repo();
    fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(dir.path(), "first");

    let err = blast_since(dir.path(), "no-such-ref").unwrap_err();
    assert!(
        err.contains("no-such-ref") && err.contains("valid ref"),
        "{err}"
    );
}

/// End-to-end through the compiled command path: a moved `main` produces
/// a comment covering only the PR's own changes (M2.12 acceptance).
#[test]
fn blast_comment_covers_only_the_prs_own_changes() {
    let fx = PrFixture::new();
    let root = fx.dir.path();

    // Index the graph at the PR HEAD.
    fs::create_dir_all(&fx.weave_dir).unwrap();
    let files = ["core.rs", "feature.rs"]
        .iter()
        .map(|f| root.join(f))
        .collect::<Vec<_>>();
    crate::index::full_reindex(root, &fx.weave_dir, &fx.active_db, &files).unwrap();

    let out_path = root.join("blast.md");
    cmd_blast(root, "main", "md", Some(&out_path)).unwrap();
    let md = fs::read_to_string(&out_path).unwrap();

    assert!(md.contains("`main`...`HEAD`"), "{md}");
    assert!(md.contains("feature.rs"), "{md}");
    assert!(
        md.contains("feature_caller"),
        "downstream symbol listed: {md}"
    );
    assert!(
        !md.contains("unrelated"),
        "main's own commit must not appear: {md}"
    );
}

#[test]
fn blast_json_format_is_parseable_and_carries_the_impacted_set() {
    let fx = PrFixture::new();
    let root = fx.dir.path();

    fs::create_dir_all(&fx.weave_dir).unwrap();
    let files = ["core.rs", "feature.rs"]
        .iter()
        .map(|f| root.join(f))
        .collect::<Vec<_>>();
    crate::index::full_reindex(root, &fx.weave_dir, &fx.active_db, &files).unwrap();

    let out_path = root.join("blast.json");
    cmd_blast(root, "main", "json", Some(&out_path)).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out_path).unwrap()).unwrap();
    assert_eq!(parsed["base"], "main");
    assert!(parsed["impacted_count"].as_u64().unwrap() >= 1);
    let symbols: Vec<&str> = parsed["impacted_symbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["symbol"].as_str().unwrap())
        .collect();
    assert!(symbols.contains(&"feature_caller"), "{parsed}");
}

#[test]
fn blast_with_an_unknown_format_is_a_clear_error() {
    let fx = PrFixture::new();
    assert!(cmd_blast(fx.dir.path(), "main", "xml", None).is_err());
}
