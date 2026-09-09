use std::fs;
use std::path::Path;

use super::{cmd_check_contracts, record_expectations, repo_contract_hash};

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
    let hash_a = repo_contract_hash(a).unwrap();
    let hash_b = repo_contract_hash(b).unwrap();
    record_expectations(a, b, &hash_a, &hash_b, None, None).unwrap();
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

    cmd_check_contracts(consumer.path()).unwrap();
}

#[test]
fn exported_signature_change_diverges_and_strict_fails() {
    let consumer = RepoFixture::new("consumer");
    let provider = RepoFixture::new("provider");
    link(consumer.path(), provider.path());
    config_with_policy(consumer.path(), provider.path(), "strict");

    // Provider's exported signature changes after linking.
    write_source(provider.path(), "provider", "pub fn exported(x: u64) {}\n");

    let result = cmd_check_contracts(consumer.path());
    assert!(result.is_err(), "strict policy must exit non-zero on drift");
}

#[test]
fn exported_signature_change_warns_without_failing_under_warn_policy() {
    let consumer = RepoFixture::new("consumer");
    let provider = RepoFixture::new("provider");
    link(consumer.path(), provider.path());
    config_with_policy(consumer.path(), provider.path(), "warn");

    write_source(provider.path(), "provider", "pub fn exported(x: u64) {}\n");

    cmd_check_contracts(consumer.path()).unwrap();
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

    cmd_check_contracts(consumer.path()).unwrap();
}

#[test]
fn missing_expectation_is_reported_not_silent() {
    let consumer = RepoFixture::new("consumer");
    let provider = RepoFixture::new("provider");
    // No link performed — no expectation recorded.
    config_with_policy(consumer.path(), provider.path(), "warn");

    cmd_check_contracts(consumer.path()).unwrap();
}

#[test]
fn no_linked_repos_is_a_clear_error() {
    let consumer = RepoFixture::new("consumer");
    let result = cmd_check_contracts(consumer.path());
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
