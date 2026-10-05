use std::fs;
use std::path::Path;
use std::process::Command;

use super::*;

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn commit_all(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
}

/// Minimal indexed PR fixture: a base commit plus one PR commit touching
/// `feature.rs`, which calls `core.rs`'s only export — enough for
/// `blast::compute` to report a non-empty impacted set.
struct Fixture {
    dir: tempfile::TempDir,
    weave_dir: std::path::PathBuf,
    active_db: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        git(dir.path(), &["config", "user.email", "test@example.com"]);
        git(dir.path(), &["config", "user.name", "Test"]);
        fs::write(dir.path().join("core.rs"), "pub fn core() {}\n").unwrap();
        fs::write(dir.path().join("feature.rs"), "fn feature() { core(); }\n").unwrap();
        commit_all(dir.path(), "base");
        git(dir.path(), &["checkout", "-q", "-b", "pr"]);
        fs::write(
            dir.path().join("feature.rs"),
            "fn feature() { core(); }\nfn feature2() { feature(); }\n",
        )
        .unwrap();
        commit_all(dir.path(), "pr change");

        let weave_dir = dir.path().join(".weave");
        fs::create_dir_all(&weave_dir).unwrap();
        let active_db = weave_dir.join("graph.db");
        let files = ["core.rs", "feature.rs"]
            .iter()
            .map(|f| dir.path().join(f))
            .collect::<Vec<_>>();
        crate::index::full_reindex(dir.path(), &weave_dir, &active_db, &files).unwrap();
        Self {
            dir,
            weave_dir,
            active_db,
        }
    }
}

#[test]
fn classify_risk_is_low_below_every_threshold() {
    assert_eq!(classify_risk(0, 0), RiskLevel::Low);
    assert_eq!(classify_risk(9, 0), RiskLevel::Low);
}

#[test]
fn classify_risk_is_medium_at_moderate_fanout_or_one_exported_symbol() {
    assert_eq!(classify_risk(10, 0), RiskLevel::Medium);
    assert_eq!(classify_risk(0, 1), RiskLevel::Medium);
    assert_eq!(classify_risk(49, 2), RiskLevel::Medium);
}

#[test]
fn classify_risk_is_high_at_large_fanout_or_several_exported_symbols() {
    assert_eq!(classify_risk(50, 0), RiskLevel::High);
    assert_eq!(classify_risk(0, 3), RiskLevel::High);
}

#[test]
fn classify_risk_is_critical_only_when_exported_and_large_fanout_combine() {
    assert_eq!(classify_risk(50, 1), RiskLevel::Critical);
    assert_eq!(classify_risk(1000, 5), RiskLevel::Critical);
    // Large fan-out alone, nothing exported, stays High — not Critical.
    assert_eq!(classify_risk(1000, 0), RiskLevel::High);
}

#[test]
fn cmd_pr_review_markdown_includes_the_risk_header_and_blast_body() {
    let fx = Fixture::new();
    let out_path = fx.dir.path().join("pr-review.md");
    cmd_pr_review(
        fx.dir.path(),
        "main",
        "md",
        Some(&out_path),
        "2",
        "callers",
        "never",
        &[],
        None,
        None,
        None,
        &[],
    )
    .unwrap();
    let text = fs::read_to_string(&out_path).unwrap();
    assert!(text.starts_with("## Weave PR Review"), "{text}");
    assert!(text.contains("**Blast radius:"), "{text}");
    assert!(text.contains("feature2"), "{text}");
}

#[test]
fn cmd_pr_review_json_carries_a_risk_field_alongside_blast_data() {
    let fx = Fixture::new();
    let out_path = fx.dir.path().join("pr-review.json");
    cmd_pr_review(
        fx.dir.path(),
        "main",
        "json",
        Some(&out_path),
        "2",
        "callers",
        "never",
        &[],
        None,
        None,
        None,
        &[],
    )
    .unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out_path).unwrap()).unwrap();
    assert!(parsed["risk"].is_string(), "{parsed}");
    assert_eq!(parsed["base"], "main");
    assert!(parsed["impacted_count"].as_u64().unwrap() >= 1);
}

#[test]
fn parse_fail_on_accepts_every_documented_value() {
    assert_eq!(
        Severity::parse_fail_on("blocker"),
        Ok(Some(Severity::Blocker))
    );
    assert_eq!(
        Severity::parse_fail_on("warning"),
        Ok(Some(Severity::Warning))
    );
    assert_eq!(Severity::parse_fail_on("info"), Ok(Some(Severity::Info)));
    assert_eq!(Severity::parse_fail_on("never"), Ok(None));
}

#[test]
fn parse_fail_on_rejects_an_unknown_value() {
    let err = Severity::parse_fail_on("yolo").unwrap_err();
    assert!(err.contains("unknown --fail-on value"), "{err}");
}

fn synthetic_report(impacted_count: usize) -> blast::BlastReport {
    blast::BlastReport {
        base: "main".to_string(),
        head: "HEAD".to_string(),
        changed_files: Vec::new(),
        impacted: (0..impacted_count)
            .map(|i| (format!("sym{i}"), "f.rs".to_string(), 1))
            .collect(),
        folded: false,
        modules: Vec::new(),
        exported_touched: Vec::new(),
    }
}

#[test]
fn oversized_blast_finding_is_none_below_the_high_fanout_threshold() {
    assert!(oversized_blast_finding(&synthetic_report(HIGH_FANOUT - 1)).is_none());
}

#[test]
fn oversized_blast_finding_is_a_warning_at_the_high_fanout_threshold() {
    let finding = oversized_blast_finding(&synthetic_report(HIGH_FANOUT)).unwrap();
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.id, "oversized-blast-radius");
}

fn init_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    git(dir.path(), &["config", "user.name", "Test"]);
    dir
}

fn index_all(root: &Path, files: &[&str]) {
    let weave_dir = root.join(".weave");
    fs::create_dir_all(&weave_dir).unwrap();
    let active_db = weave_dir.join("graph.db");
    let paths: Vec<_> = files.iter().map(|f| root.join(f)).collect();
    crate::index::full_reindex(root, &weave_dir, &active_db, &paths).unwrap();
}

#[test]
fn phantom_symbol_reference_is_a_warning_that_does_not_fail_by_default() {
    let dir = init_repo();
    fs::write(dir.path().join("core.rs"), "pub fn core() {}\n").unwrap();
    fs::write(dir.path().join("feature.rs"), "fn feature() { core(); }\n").unwrap();
    commit_all(dir.path(), "base");
    git(dir.path(), &["checkout", "-q", "-b", "pr"]);
    fs::write(
        dir.path().join("feature.rs"),
        "fn feature() { core(); totally_undefined_fn(); }\n",
    )
    .unwrap();
    commit_all(dir.path(), "pr change");
    index_all(dir.path(), &["core.rs", "feature.rs"]);

    let out_path = dir.path().join("pr-review.md");
    cmd_pr_review(
        dir.path(),
        "main",
        "md",
        Some(&out_path),
        "2",
        "callers",
        "blocker",
        &[],
        None,
        None,
        None,
        &[],
    )
    .unwrap();
    let text = fs::read_to_string(&out_path).unwrap();
    assert!(text.contains("[warning]"), "{text}");
    assert!(text.contains("totally_undefined_fn"), "{text}");
}

#[cfg(feature = "policy-lint")]
#[test]
fn policy_violation_is_a_blocker_that_fails_by_default_and_can_be_waived() {
    let dir = init_repo();
    fs::write(dir.path().join("core.rs"), "pub fn core() {}\n").unwrap();
    fs::write(dir.path().join("feature.rs"), "fn feature() { core(); }\n").unwrap();
    fs::create_dir_all(dir.path().join(".weave")).unwrap();
    fs::write(
        dir.path().join(".weave").join("policy.yaml"),
        "rules:\n  - disallow:\n      from: \"feature.rs\"\n      to: \"core.rs\"\n",
    )
    .unwrap();
    commit_all(dir.path(), "base");
    git(dir.path(), &["checkout", "-q", "-b", "pr"]);
    fs::write(
        dir.path().join("feature.rs"),
        "fn feature() { core(); }\nfn feature2() { feature(); }\n",
    )
    .unwrap();
    commit_all(dir.path(), "pr change");
    index_all(dir.path(), &["core.rs", "feature.rs"]);

    let out_path = dir.path().join("pr-review.md");
    let err = cmd_pr_review(
        dir.path(),
        "main",
        "md",
        Some(&out_path),
        "2",
        "callers",
        "blocker",
        &[],
        None,
        None,
        None,
        &[],
    )
    .unwrap_err();
    assert!(err.to_string().contains("unwaived finding"), "{err}");
    let text = fs::read_to_string(&out_path).unwrap();
    assert!(text.contains("[blocker]"), "{text}");

    let waive_no_reason = cmd_pr_review(
        dir.path(),
        "main",
        "md",
        Some(&out_path),
        "2",
        "callers",
        "blocker",
        &["policy:disallow:feature.rs->core.rs".to_string()],
        None,
        None,
        None,
        &[],
    )
    .unwrap_err();
    assert!(
        waive_no_reason.to_string().contains("--reason is required"),
        "{waive_no_reason}"
    );

    cmd_pr_review(
        dir.path(),
        "main",
        "md",
        Some(&out_path),
        "2",
        "callers",
        "blocker",
        &["policy:disallow:feature.rs->core.rs".to_string()],
        Some("policy predates this PR"),
        None,
        None,
        &[],
    )
    .unwrap();
    let waived_text = fs::read_to_string(&out_path).unwrap();
    assert!(waived_text.contains("_(waived)_"), "{waived_text}");
}

#[test]
fn linked_provider_contract_drift_is_a_blocker_finding() {
    let consumer = init_repo();
    let provider = tempfile::tempdir().unwrap();
    fs::write(
        provider.path().join("provider.rs"),
        "pub fn exported(x: u32) {}\n",
    )
    .unwrap();
    let provider_weave = provider.path().join(".weave");
    fs::create_dir_all(&provider_weave).unwrap();
    crate::index::full_reindex(
        provider.path(),
        &provider_weave,
        &provider_weave.join("graph.db"),
        &[provider.path().join("provider.rs")],
    )
    .unwrap();

    fs::write(consumer.path().join("core.rs"), "pub fn core() {}\n").unwrap();
    fs::write(
        consumer.path().join("feature.rs"),
        "fn feature() { core(); }\n",
    )
    .unwrap();
    commit_all(consumer.path(), "base");
    git(consumer.path(), &["checkout", "-q", "-b", "pr"]);
    fs::write(
        consumer.path().join("feature.rs"),
        "fn feature() { core(); }\nfn feature2() { feature(); }\n",
    )
    .unwrap();
    commit_all(consumer.path(), "pr change");
    index_all(consumer.path(), &["core.rs", "feature.rs"]);

    let provider_map = crate::contracts::repo_contract_map(provider.path()).unwrap();
    let consumer_map = crate::contracts::repo_contract_map(consumer.path()).unwrap();
    crate::contracts::record_expectations(
        consumer.path(),
        provider.path(),
        &consumer_map,
        &provider_map,
        None,
        None,
    )
    .unwrap();
    fs::write(
        consumer.path().join(".weave").join("config.toml"),
        format!(
            "[federation]\nlinked_repos = [\"{}\"]\n",
            provider.path().display()
        ),
    )
    .unwrap();

    // Provider's exported signature changes after linking — the consumer's
    // recorded expectation is now stale.
    fs::write(
        provider.path().join("provider.rs"),
        "pub fn exported(x: u64) {}\n",
    )
    .unwrap();

    let out_path = consumer.path().join("pr-review.md");
    let err = cmd_pr_review(
        consumer.path(),
        "main",
        "md",
        Some(&out_path),
        "2",
        "callers",
        "blocker",
        &[],
        None,
        None,
        None,
        &[],
    )
    .unwrap_err();
    assert!(err.to_string().contains("unwaived finding"), "{err}");
    let text = fs::read_to_string(&out_path).unwrap();
    assert!(text.contains("contract diverged"), "{text}");
}

#[test]
fn fail_on_never_never_fails_even_with_a_blocker_finding() {
    let consumer = init_repo();
    let provider = tempfile::tempdir().unwrap();
    fs::write(
        provider.path().join("provider.rs"),
        "pub fn exported(x: u32) {}\n",
    )
    .unwrap();
    let provider_weave = provider.path().join(".weave");
    fs::create_dir_all(&provider_weave).unwrap();
    crate::index::full_reindex(
        provider.path(),
        &provider_weave,
        &provider_weave.join("graph.db"),
        &[provider.path().join("provider.rs")],
    )
    .unwrap();

    fs::write(consumer.path().join("core.rs"), "pub fn core() {}\n").unwrap();
    fs::write(
        consumer.path().join("feature.rs"),
        "fn feature() { core(); }\n",
    )
    .unwrap();
    commit_all(consumer.path(), "base");
    git(consumer.path(), &["checkout", "-q", "-b", "pr"]);
    fs::write(
        consumer.path().join("feature.rs"),
        "fn feature() { core(); }\nfn feature2() { feature(); }\n",
    )
    .unwrap();
    commit_all(consumer.path(), "pr change");
    index_all(consumer.path(), &["core.rs", "feature.rs"]);

    let provider_map = crate::contracts::repo_contract_map(provider.path()).unwrap();
    let consumer_map = crate::contracts::repo_contract_map(consumer.path()).unwrap();
    crate::contracts::record_expectations(
        consumer.path(),
        provider.path(),
        &consumer_map,
        &provider_map,
        None,
        None,
    )
    .unwrap();
    fs::write(
        consumer.path().join(".weave").join("config.toml"),
        format!(
            "[federation]\nlinked_repos = [\"{}\"]\n",
            provider.path().display()
        ),
    )
    .unwrap();
    fs::write(
        provider.path().join("provider.rs"),
        "pub fn exported(x: u64) {}\n",
    )
    .unwrap();

    let out_path = consumer.path().join("pr-review.md");
    cmd_pr_review(
        consumer.path(),
        "main",
        "md",
        Some(&out_path),
        "2",
        "callers",
        "never",
        &[],
        None,
        None,
        None,
        &[],
    )
    .unwrap();
}

#[test]
fn cmd_pr_review_rejects_an_unknown_format() {
    let fx = Fixture::new();
    let err = cmd_pr_review(
        fx.dir.path(),
        "main",
        "yaml",
        None,
        "2",
        "callers",
        "never",
        &[],
        None,
        None,
        None,
        &[],
    )
    .unwrap_err();
    assert!(err.to_string().contains("unknown format"), "{err}");
    let _ = &fx.weave_dir;
    let _ = &fx.active_db;
}

/// A 3-branch stack off `main`: `branch-a` adds `file_a.rs`, `branch-b`
/// (checked out, HEAD) adds `file_b.rs` on top of it — one distinct file
/// per branch, so file-level precision can't blur which branch owns which
/// symbol the way editing one shared file across branches would.
struct StackFixture {
    dir: tempfile::TempDir,
}

impl StackFixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        git(dir.path(), &["config", "user.email", "test@example.com"]);
        git(dir.path(), &["config", "user.name", "Test"]);
        fs::write(dir.path().join("core.rs"), "pub fn core() {}\n").unwrap();
        commit_all(dir.path(), "base");
        git(dir.path(), &["checkout", "-q", "-b", "branch-a"]);
        fs::write(dir.path().join("file_a.rs"), "fn feature_a() { core(); }\n").unwrap();
        commit_all(dir.path(), "branch-a work");
        git(dir.path(), &["checkout", "-q", "-b", "branch-b"]);
        fs::write(dir.path().join("file_b.rs"), "fn feature_b() { core(); }\n").unwrap();
        commit_all(dir.path(), "branch-b work");

        let weave_dir = dir.path().join(".weave");
        fs::create_dir_all(&weave_dir).unwrap();
        let active_db = weave_dir.join("graph.db");
        let files = ["core.rs", "file_a.rs", "file_b.rs"]
            .iter()
            .map(|f| dir.path().join(f))
            .collect::<Vec<_>>();
        crate::index::full_reindex(dir.path(), &weave_dir, &active_db, &files).unwrap();
        Self { dir }
    }
}

#[test]
fn stack_base_scores_only_this_branchs_own_diff_not_the_whole_stack() {
    let fx = StackFixture::new();
    let out_path = fx.dir.path().join("pr-review.md");
    cmd_pr_review(
        fx.dir.path(),
        "main",
        "md",
        Some(&out_path),
        "2",
        "callers",
        "never",
        &[],
        None,
        None,
        Some("branch-a"),
        &[],
    )
    .unwrap();
    let text = fs::read_to_string(&out_path).unwrap();
    assert!(text.contains("feature_b"), "{text}");
    assert!(!text.contains("feature_a"), "{text}");
    assert!(text.contains("Stack context"), "{text}");
    assert!(text.contains("stack parent `branch-a`"), "{text}");
    assert!(text.contains("merge target `main`"), "{text}");
}

#[test]
fn stack_base_auto_detects_the_nearest_ancestor_branch() {
    let fx = StackFixture::new();
    let out_path = fx.dir.path().join("pr-review.md");
    cmd_pr_review(
        fx.dir.path(),
        "main",
        "md",
        Some(&out_path),
        "2",
        "callers",
        "never",
        &[],
        None,
        None,
        Some("auto"),
        &[],
    )
    .unwrap();
    let text = fs::read_to_string(&out_path).unwrap();
    assert!(text.contains("feature_b"), "{text}");
    assert!(!text.contains("feature_a"), "{text}");
    assert!(text.contains("stack parent `branch-a`"), "{text}");
}

#[test]
fn stack_base_auto_errors_clearly_with_no_ancestor_branch() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "solo"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    git(dir.path(), &["config", "user.name", "Test"]);
    fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    commit_all(dir.path(), "first");
    index_all(dir.path(), &["a.rs"]);

    let err = cmd_pr_review(
        dir.path(),
        "solo",
        "md",
        None,
        "2",
        "callers",
        "never",
        &[],
        None,
        None,
        Some("auto"),
        &[],
    )
    .unwrap_err();
    assert!(err.to_string().contains("--stack-base auto"), "{err}");
}

#[test]
fn stack_base_json_carries_stack_base_and_cumulative_fields() {
    let fx = StackFixture::new();
    let out_path = fx.dir.path().join("pr-review.json");
    cmd_pr_review(
        fx.dir.path(),
        "main",
        "json",
        Some(&out_path),
        "2",
        "callers",
        "never",
        &[],
        None,
        None,
        Some("branch-a"),
        &[],
    )
    .unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out_path).unwrap()).unwrap();
    assert_eq!(parsed["stack_base"], "branch-a");
    assert_eq!(parsed["base"], "branch-a");
    let own_count = parsed["impacted_count"].as_u64().unwrap();
    let cumulative_count = parsed["cumulative"]["impacted_count"].as_u64().unwrap();
    assert!(
        cumulative_count > own_count,
        "cumulative (vs main) must cover more than this branch's own diff (vs branch-a): {parsed}"
    );
}

#[test]
fn lane_scores_a_ref_independently_alongside_the_main_report() {
    let fx = StackFixture::new();
    let out_path = fx.dir.path().join("pr-review.md");
    cmd_pr_review(
        fx.dir.path(),
        "main",
        "md",
        Some(&out_path),
        "2",
        "callers",
        "never",
        &[],
        None,
        None,
        None,
        &["branch-a".to_string()],
    )
    .unwrap();
    let text = fs::read_to_string(&out_path).unwrap();
    assert!(text.contains("### Lanes"), "{text}");
    assert!(text.contains("`branch-a`"), "{text}");
    assert!(text.contains("feature_a"), "{text}");
    assert!(text.contains("feature_b"), "{text}");
}

#[test]
fn lane_json_includes_a_lanes_array_with_per_lane_risk() {
    let fx = StackFixture::new();
    let out_path = fx.dir.path().join("pr-review.json");
    cmd_pr_review(
        fx.dir.path(),
        "main",
        "json",
        Some(&out_path),
        "2",
        "callers",
        "never",
        &[],
        None,
        None,
        None,
        &["branch-a".to_string()],
    )
    .unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out_path).unwrap()).unwrap();
    let lanes = parsed["lanes"].as_array().unwrap();
    assert_eq!(lanes.len(), 1, "{parsed}");
    assert_eq!(lanes[0]["lane"], "branch-a");
    assert!(lanes[0]["risk"].is_string(), "{parsed}");
}

#[test]
fn no_stack_base_or_lanes_leaves_output_unchanged_from_before_the_flags_existed() {
    let fx = Fixture::new();
    let out_path = fx.dir.path().join("pr-review.md");
    cmd_pr_review(
        fx.dir.path(),
        "main",
        "md",
        Some(&out_path),
        "2",
        "callers",
        "never",
        &[],
        None,
        None,
        None,
        &[],
    )
    .unwrap();
    let text = fs::read_to_string(&out_path).unwrap();
    assert!(!text.contains("Stack context"), "{text}");
    assert!(!text.contains("### Lanes"), "{text}");
}
