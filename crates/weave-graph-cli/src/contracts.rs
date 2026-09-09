//! Boundary contract hashing (`impl.md` M2.2, `plan.md` §2.3): `weave link`
//! records each repo's whole-repo contract hash as the *other* repo's
//! expectation in the `contracts` table; `weave check-contracts` recomputes
//! the current hash of every linked repo and compares. Divergence — never
//! elapsed time — is the staleness signal, resolved by `staleness_policy`
//! (`warn` diagnostics, `strict` non-zero exit for CI, `ignore` silence).

use std::path::{Path, PathBuf};

use weave_graph_parse::{Language, contract};
use weave_graph_store_sqlite::SqliteStorage;

use crate::config;
use crate::discover_files;
use crate::index::parse_all;

/// Whole-repo contract hash: every indexable file's exported signatures,
/// canonicalized and hashed. Language comes from the file's extension —
/// the same dispatch `weave index` uses.
pub(crate) fn repo_contract_hash(root: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let files = discover_files(root);
    let (_, parsed_files) = parse_all(root, &files);
    let mut entries = Vec::new();
    for (path, parsed) in &parsed_files {
        let rel = match path.strip_prefix(root) {
            Ok(rel) => rel,
            Err(_) => path.as_path(),
        };
        let Some(language) = Language::from_path(rel) else {
            continue;
        };
        entries.extend(contract::exported_entries(language, parsed));
    }
    Ok(contract::hash_entries(entries))
}

fn config_path(root: &Path) -> PathBuf {
    root.join(".weave").join("config.toml")
}

fn linked_repos(root: &Path) -> Vec<PathBuf> {
    config::read_linked_repos(&config_path(root))
        .into_iter()
        .map(|p| if p.is_absolute() { p } else { root.join(p) })
        .collect()
}

fn staleness_policy(root: &Path) -> String {
    config::get_key(&config_path(root), "federation.staleness_policy")
        .unwrap_or_else(|| "warn".to_string())
}

/// `weave link` side: record both repos' current hashes as each other's
/// expectation, so each repo's own `graph.db` knows what the other looked
/// like at link time.
pub(crate) fn record_expectations(
    repo_a: &Path,
    repo_b: &Path,
    hash_a: &str,
    hash_b: &str,
    sha_a: Option<&str>,
    sha_b: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    record_one_side(repo_a, repo_b, hash_b, sha_b)?;
    record_one_side(repo_b, repo_a, hash_a, sha_a)?;
    Ok(())
}

fn record_one_side(
    consumer_root: &Path,
    provider_root: &Path,
    provider_hash: &str,
    provider_sha: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = crate::open_storage_for_read(consumer_root)?.1;
    let mut storage = SqliteStorage::open(&db_path)?;
    let consumer_label = repo_label(consumer_root);
    let provider_label = repo_label(provider_root);
    let sha = provider_sha
        .map(str::to_string)
        .or_else(|| crate::git::current_sha(provider_root))
        .unwrap_or_else(|| "uncommitted".to_string());
    storage.upsert_contract(&consumer_label, &provider_label, provider_hash, &sha)?;
    Ok(())
}

fn repo_label(root: &Path) -> String {
    root.canonicalize()
        .unwrap_or_else(|_| root.to_path_buf())
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| root.to_string_lossy().to_string())
}

/// `weave check-contracts`: recompute each linked repo's current contract
/// hash and compare against the expectation recorded at link time.
pub(crate) fn cmd_check_contracts(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let linked = linked_repos(root);
    if linked.is_empty() {
        return Err(
            "No linked repos in .weave/config.toml ([federation] linked_repos). \
             Run `weave link <repo-a> <repo-b>` first."
                .into(),
        );
    }
    let policy = staleness_policy(root);
    let consumer_label = repo_label(root);
    let (storage, _) = crate::open_storage_for_read(root)?;
    let expectations = storage.contract_expectations(&consumer_label)?;

    let mut diverged = 0usize;
    let mut missing = 0usize;
    let mut current = 0usize;

    for provider in &linked {
        let provider_label = repo_label(provider);
        let expected = expectations
            .iter()
            .find(|(name, _, _)| *name == provider_label);
        let Some((_, expected_hash, expected_sha)) = expected else {
            missing += 1;
            println!(
                "⚠️ No recorded contract expectation for '{provider_label}' — run `weave link`."
            );
            continue;
        };

        let actual = repo_contract_hash(provider)?;
        if actual == *expected_hash {
            current += 1;
            println!("✓ {provider_label}: contract up to date");
            continue;
        }

        diverged += 1;
        let detail = format!(
            "⚠️ Contract drift detected in '{provider_label}' (expected {expected_sha}, \
             recorded hash {expected_hash}; current hash {actual})"
        );
        match policy.as_str() {
            "ignore" => {}
            "strict" => println!("{detail}"),
            // "warn" and any unknown value: diagnostic, never blocking.
            _ => println!("{detail}"),
        }
    }

    println!(
        "{current} up to date, {diverged} diverged, {missing} without expectation \
         (policy: {policy})"
    );
    if diverged > 0 && policy == "strict" {
        return Err("Contract divergence detected under staleness_policy = strict".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
