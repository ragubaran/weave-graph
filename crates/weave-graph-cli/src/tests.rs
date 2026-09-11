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

fn snippet_cache_key() -> String {
    ci_cache_snippet()
        .lines()
        .map(str::trim_start)
        .find(|l| l.starts_with("key:"))
        .expect("snippet defines a cache key")
        .trim_start_matches("key:")
        .trim()
        .to_string()
}

fn snippet_restore_keys() -> Vec<String> {
    let mut keys = Vec::new();
    let mut in_block = false;
    for line in ci_cache_snippet().lines() {
        if line.trim_start().starts_with("restore-keys:") {
            in_block = true;
        } else if in_block {
            if line.starts_with("      ") && !line.trim().is_empty() {
                keys.push(line.trim().to_string());
            } else {
                break;
            }
        }
    }
    assert!(!keys.is_empty(), "restore-keys block must be non-empty");
    keys
}

/// Minimal actions/cache restore semantics: exact key first, then the
/// restore-keys prefixes in declared order, newest matching cache wins.
fn simulate_cache_restore(
    caches: &[(String, String)],
    key: &str,
    restore_keys: &[String],
) -> Option<String> {
    if let Some((_, content)) = caches.iter().find(|(k, _)| k == key) {
        return Some(content.clone());
    }
    for prefix in restore_keys {
        if let Some((_, content)) = caches.iter().rev().find(|(k, _)| k.starts_with(prefix)) {
            return Some(content.clone());
        }
    }
    None
}

fn instantiate(template: &str, os: &str, ref_name: &str, sha: &str) -> String {
    template
        .replace("${{ runner.os }}", os)
        .replace("${{ github.ref_name }}", ref_name)
        .replace("${{ github.sha }}", sha)
}

#[test]
fn ci_cache_snippet_key_uses_exact_sha_with_prefix_fallback() {
    let key = snippet_cache_key();
    assert!(
        key.contains("${{ github.sha }}"),
        "key must pin the exact sha: {key}"
    );
    let restore_keys = snippet_restore_keys();
    // The first restore key is the sha-stripped key, so a sha-miss on every
    // commit still prefix-matches the newest cache for this branch.
    let stripped = key.replace("-${{ github.sha }}", "-");
    assert_eq!(restore_keys[0], stripped);
}

#[test]
fn ci_cache_snippet_restore_keys_order_prefers_branch_then_main() {
    let restore_keys = snippet_restore_keys();
    assert!(restore_keys[0].contains("${{ github.ref_name }}"));
    assert!(restore_keys[1].ends_with("-main-"));
    assert!(restore_keys.len() == 2);
}

#[test]
fn ci_cache_snippet_documents_fetch_depth_and_crossover_caveat() {
    let snippet = ci_cache_snippet();
    assert!(snippet.contains("fetch-depth: 0"));
    assert!(snippet.contains("zstd"));
    assert!(snippet.contains("skip caching"));
}

#[test]
fn generated_snippet_restores_cache_on_second_run() {
    let key = snippet_cache_key();
    // The querying branch is "feature": the branch-scoped prefix must
    // instantiate against it; the main fallback contains no ref template.
    let restore_keys: Vec<String> = snippet_restore_keys()
        .iter()
        .map(|k| instantiate(k, "Linux", "feature", "unused"))
        .collect();
    let caches: Vec<(String, String)> = Vec::new();

    // Run 1: cold — no exact match, no prefix match, nothing to restore.
    let run1_key = instantiate(&key, "Linux", "feature", "aaa");
    assert!(simulate_cache_restore(&caches, &run1_key, &restore_keys).is_none());

    // Run 1 saves under its exact sha before finishing.
    let mut caches = vec![(run1_key, "graph-aaa".to_string())];

    // Run 2: new sha → exact miss, but the prefix fallback restores run 1's
    // cache; incremental indexing then pays only the delta.
    let run2_key = instantiate(&key, "Linux", "feature", "bbb");
    assert_eq!(
        simulate_cache_restore(&caches, &run2_key, &restore_keys).as_deref(),
        Some("graph-aaa"),
        "second run must take the restore path, not cold-index"
    );

    // Every commit writes its own exact-sha entry for the next run.
    caches.push((run2_key, "graph-bbb".to_string()));
    let run3_key = instantiate(&key, "Linux", "feature", "ccc");
    assert_eq!(
        simulate_cache_restore(&caches, &run3_key, &restore_keys).as_deref(),
        Some("graph-bbb")
    );
}

#[test]
fn restore_keys_fall_back_to_main_when_branch_has_no_cache() {
    let key = snippet_cache_key();
    let restore_keys: Vec<String> = snippet_restore_keys()
        .iter()
        .map(|k| instantiate(k, "Linux", "feature", "unused"))
        .collect();
    // Fresh branch: only a cache saved from main exists. The branch-scoped
    // prefix misses, the main fallback catches it.
    let caches = vec![(
        instantiate(&key, "Linux", "main", "old"),
        "main-graph".to_string(),
    )];
    let branch_key = instantiate(&key, "Linux", "feature", "new");
    assert_eq!(
        simulate_cache_restore(&caches, &branch_key, &restore_keys).as_deref(),
        Some("main-graph")
    );
}

#[test]
fn restore_keys_order_prefers_newest_branch_cache_over_main() {
    let key = snippet_cache_key();
    let restore_keys: Vec<String> = snippet_restore_keys()
        .iter()
        .map(|k| instantiate(k, "Linux", "feature", "unused"))
        .collect();
    // Main saved first, the same branch saved later: the branch-scoped
    // prefix must win over the main fallback even though main is also a
    // prefix match for the branch entries.
    let caches = vec![
        (
            instantiate(&key, "Linux", "main", "old"),
            "main-graph".to_string(),
        ),
        (
            instantiate(&key, "Linux", "feature", "mid"),
            "branch-graph".to_string(),
        ),
    ];
    let key_now = instantiate(&key, "Linux", "feature", "new");
    assert_eq!(
        simulate_cache_restore(&caches, &key_now, &restore_keys).as_deref(),
        Some("branch-graph")
    );
}
