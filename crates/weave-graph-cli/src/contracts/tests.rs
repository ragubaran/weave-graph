use std::fs;
use std::path::Path;

use super::{
    CheckContractsWaiver, cmd_check_contracts, cmd_check_contracts_submodules, record_expectations,
    repo_contract_hash, repo_contract_map,
};

struct RepoFixture {
    root: tempfile::TempDir,
}

impl RepoFixture {
    fn new(name: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let weave_dir = dir.path().join(".weave");
        fs::create_dir_all(&weave_dir).unwrap();
        let active_db = weave_dir.join("graph.db");
        let files = vec![write_source(
            dir.path(),
            name,
            "pub fn exported(x: u32) {}\n",
        )];
        crate::index::full_reindex(dir.path(), &weave_dir, &active_db, &files).unwrap();
        Self { root: dir }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }
}

fn write_source(root: &Path, name: &str, source: &str) -> std::path::PathBuf {
    let path = root.join(format!("{name}.rs"));
    fs::write(&path, source).unwrap();
    path
}

fn link(a: &Path, b: &Path) {
    let map_a = repo_contract_map(a).unwrap();
    let map_b = repo_contract_map(b).unwrap();
    record_expectations(a, b, &map_a, &map_b, None, None).unwrap();
}

fn config_with_policy(root: &Path, other: &Path, policy: &str) {
    fs::write(
        root.join(".weave").join("config.toml"),
        format!(
            "[federation]\nlinked_repos = [\"{}\"]\nstaleness_policy = \"{policy}\"\n",
            other.display()
        ),
    )
    .unwrap();
}

#[test]
fn up_to_date_contract_passes() {
    let consumer = RepoFixture::new("consumer");
    let provider = RepoFixture::new("provider");
    link(consumer.path(), provider.path());
    config_with_policy(consumer.path(), provider.path(), "warn");

    cmd_check_contracts(
        consumer.path(),
        false,
        false,
        CheckContractsWaiver::default(),
    )
    .unwrap();
}

#[test]
fn exported_signature_change_diverges_and_strict_fails() {
    let consumer = RepoFixture::new("consumer");
    let provider = RepoFixture::new("provider");
    link(consumer.path(), provider.path());
    config_with_policy(consumer.path(), provider.path(), "strict");

    // Provider's exported signature changes after linking.
    write_source(provider.path(), "provider", "pub fn exported(x: u64) {}\n");

    let result = cmd_check_contracts(
        consumer.path(),
        false,
        false,
        CheckContractsWaiver::default(),
    );
    assert!(result.is_err(), "strict policy must exit non-zero on drift");
}

#[test]
fn exported_signature_change_warns_without_failing_under_warn_policy() {
    let consumer = RepoFixture::new("consumer");
    let provider = RepoFixture::new("provider");
    link(consumer.path(), provider.path());
    config_with_policy(consumer.path(), provider.path(), "warn");

    write_source(provider.path(), "provider", "pub fn exported(x: u64) {}\n");

    cmd_check_contracts(
        consumer.path(),
        false,
        false,
        CheckContractsWaiver::default(),
    )
    .unwrap();
}

#[test]
fn private_change_stays_up_to_date() {
    let consumer = RepoFixture::new("consumer");
    let provider = RepoFixture::new("provider");
    link(consumer.path(), provider.path());
    config_with_policy(consumer.path(), provider.path(), "strict");

    // Private helper added/renamed: not part of the exported contract.
    write_source(
        provider.path(),
        "provider",
        "pub fn exported(x: u32) {}\nfn renamed_private(y: u64) {}\n",
    );

    cmd_check_contracts(
        consumer.path(),
        false,
        false,
        CheckContractsWaiver::default(),
    )
    .unwrap();
}

#[test]
fn missing_expectation_is_reported_not_silent() {
    let consumer = RepoFixture::new("consumer");
    let provider = RepoFixture::new("provider");
    // No link performed — no expectation recorded.
    config_with_policy(consumer.path(), provider.path(), "warn");

    cmd_check_contracts(
        consumer.path(),
        false,
        false,
        CheckContractsWaiver::default(),
    )
    .unwrap();
}

#[test]
fn no_linked_repos_is_a_clear_error() {
    let consumer = RepoFixture::new("consumer");
    let result = cmd_check_contracts(
        consumer.path(),
        false,
        false,
        CheckContractsWaiver::default(),
    );
    let message = result.unwrap_err().to_string();
    assert!(message.contains("No linked repos"), "got: {message}");
}

#[test]
fn repo_contract_hash_is_stable_for_unchanged_sources() {
    let repo = RepoFixture::new("stable");
    let first = repo_contract_hash(repo.path()).unwrap();
    let second = repo_contract_hash(repo.path()).unwrap();
    assert_eq!(first, second);
}

/// `--scoped` blocks when the changed symbol is one the consumer actually
/// calls — a real `weave link` (not the lightweight
/// `link()` helper above) so `imported_symbols` has a genuine cross-repo
/// edge to read. Rust fixtures, matching this file's own convention (and
/// proven end-to-end through the real extractor by the tests above).
#[test]
fn scoped_check_blocks_when_the_changed_symbol_is_actually_imported() {
    let consumer_dir = tempfile::tempdir().unwrap();
    let provider_dir = tempfile::tempdir().unwrap();
    let consumer_weave = consumer_dir.path().join(".weave");
    let provider_weave = provider_dir.path().join(".weave");
    fs::create_dir_all(&consumer_weave).unwrap();
    fs::create_dir_all(&provider_weave).unwrap();
    let a_rs = consumer_dir.path().join("a.rs");
    fs::write(&a_rs, "fn consumer_fn() { provider_fn(); }\n").unwrap();
    let b_rs = provider_dir.path().join("b.rs");
    fs::write(&b_rs, "pub fn provider_fn() {}\n").unwrap();
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
    config_with_policy(consumer_dir.path(), provider_dir.path(), "strict");

    fs::write(&b_rs, "pub fn provider_fn(extra: u32) {}\n").unwrap();

    let result = cmd_check_contracts(
        consumer_dir.path(),
        true,
        true,
        CheckContractsWaiver::default(),
    );
    assert!(
        result.is_err(),
        "an imported symbol's signature changed — --scoped must still block"
    );
}

/// The mirror case: a provider symbol changes that the consumer never
/// calls — `--scoped` must report it but not fail CI, even under `strict`.
#[test]
fn scoped_check_does_not_block_when_the_changed_symbol_is_not_imported() {
    let consumer_dir = tempfile::tempdir().unwrap();
    let provider_dir = tempfile::tempdir().unwrap();
    let consumer_weave = consumer_dir.path().join(".weave");
    let provider_weave = provider_dir.path().join(".weave");
    fs::create_dir_all(&consumer_weave).unwrap();
    fs::create_dir_all(&provider_weave).unwrap();
    let a_rs = consumer_dir.path().join("a.rs");
    fs::write(&a_rs, "fn consumer_fn() { provider_fn(); }\n").unwrap();
    let b_rs = provider_dir.path().join("b.rs");
    fs::write(&b_rs, "pub fn provider_fn() {}\npub fn unrelated_fn() {}\n").unwrap();
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
    config_with_policy(consumer_dir.path(), provider_dir.path(), "strict");

    // Only the symbol the consumer never calls changes.
    fs::write(
        &b_rs,
        "pub fn provider_fn() {}\npub fn unrelated_fn(extra: u32) {}\n",
    )
    .unwrap();

    let result = cmd_check_contracts(
        consumer_dir.path(),
        true,
        true,
        CheckContractsWaiver::default(),
    );
    assert!(
        result.is_ok(),
        "drift touches no imported symbol — --scoped must not fail CI: {result:?}"
    );
}

// ─── waiver mechanisms ───────────────────────────────────────────────────────

#[test]
fn weave_skip_contracts_env_var_bypasses_everything() {
    let consumer = RepoFixture::new("consumer");
    // No link, no config — a real check would error with "No linked repos".
    cmd_check_contracts(
        consumer.path(),
        false,
        false,
        CheckContractsWaiver {
            skip_env: Some("1"),
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn allow_drift_flag_without_a_reason_is_a_clear_error() {
    let consumer = RepoFixture::new("consumer");
    let provider = RepoFixture::new("provider");
    link(consumer.path(), provider.path());
    config_with_policy(consumer.path(), provider.path(), "strict");
    write_source(provider.path(), "provider", "pub fn exported(x: u64) {}\n");

    let err = cmd_check_contracts(
        consumer.path(),
        false,
        false,
        CheckContractsWaiver {
            allow_drift: true,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("--reason"), "{err}");
}

#[test]
fn allow_drift_flag_waives_strict_failure_and_is_counted_as_waived_not_diverged() {
    let consumer = RepoFixture::new("consumer");
    let provider = RepoFixture::new("provider");
    link(consumer.path(), provider.path());
    config_with_policy(consumer.path(), provider.path(), "strict");
    write_source(provider.path(), "provider", "pub fn exported(x: u64) {}\n");

    cmd_check_contracts(
        consumer.path(),
        false,
        false,
        CheckContractsWaiver {
            allow_drift: true,
            reason: Some("known break, fix incoming"),
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn allow_drift_for_only_waives_the_named_repo() {
    let consumer = RepoFixture::new("consumer");
    let provider_a = RepoFixture::new("provider_a");
    let provider_b = RepoFixture::new("provider_b");
    link(consumer.path(), provider_a.path());
    link(consumer.path(), provider_b.path());
    fs::write(
        consumer.path().join(".weave").join("config.toml"),
        format!(
            "[federation]\nlinked_repos = [\"{}\", \"{}\"]\nstaleness_policy = \"strict\"\n",
            provider_a.path().display(),
            provider_b.path().display()
        ),
    )
    .unwrap();

    // Both providers drift; only provider_a's is waived by name.
    write_source(
        provider_a.path(),
        "provider_a",
        "pub fn exported(x: u64) {}\n",
    );
    write_source(
        provider_b.path(),
        "provider_b",
        "pub fn exported(x: u64) {}\n",
    );

    let provider_a_label = provider_a
        .path()
        .canonicalize()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let err = cmd_check_contracts(
        consumer.path(),
        false,
        false,
        CheckContractsWaiver {
            allow_drift_for: Some(&provider_a_label),
            reason: Some("provider_a is a known false positive"),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("strict"),
        "provider_b's drift must still fail CI: {err}"
    );
}

#[test]
fn weave_allow_drift_repos_env_var_waives_only_listed_repos() {
    let consumer = RepoFixture::new("consumer");
    let provider = RepoFixture::new("provider");
    link(consumer.path(), provider.path());
    config_with_policy(consumer.path(), provider.path(), "strict");
    write_source(provider.path(), "provider", "pub fn exported(x: u64) {}\n");

    let provider_label = provider
        .path()
        .canonicalize()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();

    cmd_check_contracts(
        consumer.path(),
        false,
        false,
        CheckContractsWaiver {
            allow_drift_repos_env: Some(&format!("unrelated-repo,{provider_label}")),
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn weave_staleness_policy_override_env_var_downgrades_strict_to_warn() {
    let consumer = RepoFixture::new("consumer");
    let provider = RepoFixture::new("provider");
    link(consumer.path(), provider.path());
    config_with_policy(consumer.path(), provider.path(), "strict");
    write_source(provider.path(), "provider", "pub fn exported(x: u64) {}\n");

    cmd_check_contracts(
        consumer.path(),
        false,
        false,
        CheckContractsWaiver {
            staleness_override_env: Some("warn"),
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn warn_only_flag_downgrades_strict_failure_without_waiving_anything() {
    let consumer = RepoFixture::new("consumer");
    let provider = RepoFixture::new("provider");
    link(consumer.path(), provider.path());
    config_with_policy(consumer.path(), provider.path(), "strict");
    write_source(provider.path(), "provider", "pub fn exported(x: u64) {}\n");

    // No --reason required: --warn-only isn't one of the "drift flags".
    cmd_check_contracts(
        consumer.path(),
        false,
        false,
        CheckContractsWaiver {
            warn_only: true,
            ..Default::default()
        },
    )
    .unwrap();
}

#[cfg(feature = "rbac")]
#[test]
fn allow_drift_is_refused_when_the_bound_identity_lacks_the_allow_drift_role() {
    let consumer = RepoFixture::new("consumer");
    let provider = RepoFixture::new("provider");
    link(consumer.path(), provider.path());
    fs::write(
        consumer.path().join(".weave").join("config.toml"),
        format!(
            "[federation]\nlinked_repos = [\"{}\"]\nstaleness_policy = \"strict\"\n\n\
             [rbac.users]\n\"contractor-bot\" = [\"contractor\"]\n",
            provider.path().display()
        ),
    )
    .unwrap();
    write_source(provider.path(), "provider", "pub fn exported(x: u64) {}\n");

    let err = cmd_check_contracts(
        consumer.path(),
        false,
        false,
        CheckContractsWaiver {
            allow_drift: true,
            reason: Some("trying to sneak this through"),
            as_subject: Some("contractor-bot"),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("not authorized"), "{err}");
}

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

#[test]
fn check_contracts_submodules_is_ok_with_no_gitmodules_file() {
    let dir = tempfile::tempdir().unwrap();
    cmd_check_contracts_submodules(dir.path()).unwrap();
}

#[test]
fn check_contracts_submodules_errs_on_an_uninitialized_submodule() {
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

    let err = cmd_check_contracts_submodules(outer.path()).unwrap_err();
    assert!(err.to_string().contains("incomplete or blocking"), "{err}");
}

/// Sets up an outer repo with one submodule and indexes the outer repo
/// (needed now that a `Clean`/`Bumped` submodule check reads the parent's
/// own already-indexed graph for consumer-scoped drift filtering).
fn outer_with_indexed_submodule(inner_source: &str) -> (tempfile::TempDir, tempfile::TempDir) {
    let inner = init_repo();
    write_source(inner.path(), "a", inner_source);
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

    let weave_dir = outer.path().join(".weave");
    fs::create_dir_all(&weave_dir).unwrap();
    let files = vec![outer.path().join("sub").join("a.rs")];
    crate::index::full_reindex(
        outer.path(),
        &weave_dir,
        &weave_dir.join("graph.db"),
        &files,
    )
    .unwrap();

    (outer, inner)
}

#[test]
fn check_contracts_submodules_is_ok_when_every_submodule_is_clean() {
    let (outer, _inner) = outer_with_indexed_submodule("pub fn a() {}\n");
    cmd_check_contracts_submodules(outer.path()).unwrap();
}

#[test]
fn check_contracts_submodules_records_a_baseline_on_first_run_and_is_unchanged_on_the_next() {
    let (outer, _inner) = outer_with_indexed_submodule("pub fn a() {}\n");
    // First run: no prior baseline recorded yet — must not block.
    cmd_check_contracts_submodules(outer.path()).unwrap();
    // Second run against the same unchanged submodule contract: still ok.
    cmd_check_contracts_submodules(outer.path()).unwrap();
}

#[test]
fn check_contracts_submodules_blocks_when_the_parent_imports_the_drifted_symbol() {
    let (outer, inner) = outer_with_indexed_submodule("pub fn a() {}\n");
    // Parent repo actually calls the submodule's exported `a`.
    write_source(outer.path(), "consumer", "fn consumer() { a(); }\n");
    let weave_dir = outer.path().join(".weave");
    let files = vec![
        outer.path().join("sub").join("a.rs"),
        outer.path().join("consumer.rs"),
    ];
    crate::index::full_reindex(
        outer.path(),
        &weave_dir,
        &weave_dir.join("graph.db"),
        &files,
    )
    .unwrap();
    cmd_check_contracts_submodules(outer.path()).unwrap(); // records the baseline

    // Submodule's exported signature changes and the pointer is bumped.
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

    let err = cmd_check_contracts_submodules(outer.path()).unwrap_err();
    assert!(err.to_string().contains("sub"), "{err}");
}

#[test]
fn check_contracts_submodules_does_not_block_on_drift_the_parent_never_calls() {
    let (outer, inner) = outer_with_indexed_submodule("pub fn a() {}\npub fn unused() {}\n");
    cmd_check_contracts_submodules(outer.path()).unwrap(); // records the baseline

    // A symbol the parent repo never imports changes; the pointer bumps.
    write_source(
        inner.path(),
        "a",
        "pub fn a() {}\npub fn unused(extra: u32) {}\n",
    );
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

    cmd_check_contracts_submodules(outer.path()).unwrap();
}
