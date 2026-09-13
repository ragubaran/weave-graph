use std::fs;

use super::*;

#[test]
fn ensure_gitignored_creates_gitignore_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    ensure_gitignored(dir.path()).unwrap();
    let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(content.lines().any(|l| l == ".weave/*"));
    assert!(content.lines().any(|l| l == "!.weave/config.toml"));
}

#[test]
fn ensure_gitignored_appends_to_an_existing_gitignore() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
    ensure_gitignored(dir.path()).unwrap();
    let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(content.lines().any(|l| l == "target/"));
    assert!(content.lines().any(|l| l == ".weave/*"));
    assert!(content.lines().any(|l| l == "!.weave/config.toml"));
}

#[test]
fn ensure_gitignored_adds_a_trailing_newline_before_appending() {
    let dir = tempfile::tempdir().unwrap();
    // No trailing newline — the append path must add one before ".weave/*".
    fs::write(dir.path().join(".gitignore"), "target/").unwrap();
    ensure_gitignored(dir.path()).unwrap();
    let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert_eq!(content, "target/\n.weave/*\n!.weave/config.toml\n");
}

#[test]
fn ensure_gitignored_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    ensure_gitignored(dir.path()).unwrap();
    ensure_gitignored(dir.path()).unwrap();
    let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert_eq!(content.lines().filter(|l| l == &".weave/*").count(), 1);
    assert_eq!(
        content
            .lines()
            .filter(|l| l == &"!.weave/config.toml")
            .count(),
        1
    );
}

#[test]
fn ensure_gitignored_replaces_bare_weave_entry() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), ".weave\n").unwrap();
    ensure_gitignored(dir.path()).unwrap();
    let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert_eq!(content, ".weave/*\n!.weave/config.toml\n");
}

#[test]
fn ensure_ignored_updates_only_ignore_when_only_ignore_exists() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".ignore"), "custom_cache/\n").unwrap();
    ensure_ignored(dir.path()).unwrap();
    let ignore_content = fs::read_to_string(dir.path().join(".ignore")).unwrap();
    assert!(ignore_content.lines().any(|l| l == "!.weave/"));
    assert!(ignore_content.lines().any(|l| l == ".weave/*"));
    assert!(ignore_content.lines().any(|l| l == "!.weave/config.toml"));
    assert!(!dir.path().join(".gitignore").exists());
}

#[test]
fn ensure_ignored_updates_both_when_both_exist() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
    fs::write(dir.path().join(".ignore"), "!custom/\n").unwrap();
    ensure_ignored(dir.path()).unwrap();
    let gitignore_content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    let ignore_content = fs::read_to_string(dir.path().join(".ignore")).unwrap();
    assert!(gitignore_content.lines().any(|l| l == ".weave/*"));
    assert!(
        gitignore_content
            .lines()
            .any(|l| l == "!.weave/config.toml")
    );
    assert!(ignore_content.lines().any(|l| l == "!.weave/"));
    assert!(ignore_content.lines().any(|l| l == ".weave/*"));
    assert!(ignore_content.lines().any(|l| l == "!.weave/config.toml"));
}

#[test]
fn ensure_ignored_allows_config_toml_and_ignores_db() {
    let dir = tempfile::tempdir().unwrap();
    ensure_ignored(dir.path()).unwrap();
    let gitignore = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(gitignore.lines().any(|l| l == ".weave/*"));
    assert!(gitignore.lines().any(|l| l == "!.weave/config.toml"));
}

#[test]
fn ensure_mcp_configured_creates_file_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    ensure_mcp_configured(dir.path()).unwrap();
    let content = fs::read_to_string(dir.path().join(".mcp.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(
        json["mcpServers"]["weave"]["command"].as_str().unwrap(),
        "weave"
    );
}

#[test]
fn ensure_mcp_configured_preserves_existing_servers() {
    let dir = tempfile::tempdir().unwrap();
    let initial = serde_json::json!({
        "mcpServers": {
            "graft": {
                "command": "graft",
                "args": ["mcp"]
            }
        }
    });
    fs::write(
        dir.path().join(".mcp.json"),
        serde_json::to_string_pretty(&initial).unwrap(),
    )
    .unwrap();
    ensure_mcp_configured(dir.path()).unwrap();
    let content = fs::read_to_string(dir.path().join(".mcp.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(json["mcpServers"]["graft"].is_object());
    assert!(json["mcpServers"]["weave"].is_object());
}

#[test]
fn ensure_mcp_configured_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    ensure_mcp_configured(dir.path()).unwrap();
    ensure_mcp_configured(dir.path()).unwrap();
    let content = fs::read_to_string(dir.path().join(".mcp.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(json["mcpServers"]["weave"].is_object());
}

#[test]
fn try_fast_path_reports_already_up_to_date_when_shas_match_and_db_exists() {
    let dir = tempfile::tempdir().unwrap();
    let weave_dir = dir.path().join(".weave");
    fs::create_dir_all(&weave_dir).unwrap();
    let active_db = weave_dir.join("graph.db");
    fs::write(&active_db, b"db bytes").unwrap();

    let took_fast_path = try_fast_path(
        &weave_dir,
        &active_db,
        Some("sha1"),
        Some("sha1"),
        Instant::now(),
    )
    .unwrap();
    assert!(took_fast_path);
}

#[test]
fn try_fast_path_restores_from_a_cached_snapshot_for_the_current_sha() {
    let dir = tempfile::tempdir().unwrap();
    let weave_dir = dir.path().join(".weave");
    fs::create_dir_all(&weave_dir).unwrap();
    let active_db = weave_dir.join("graph.db");
    fs::write(&active_db, b"original bytes").unwrap();
    cache::save_snapshot(&weave_dir, &active_db, "sha1").unwrap();

    // A different last-indexed sha skips the "already up to date" branch,
    // so this exercises the cache-restore branch specifically.
    let took_fast_path = try_fast_path(
        &weave_dir,
        &active_db,
        Some("sha1"),
        Some("sha2"),
        Instant::now(),
    )
    .unwrap();
    assert!(took_fast_path);
    assert_eq!(
        cache::read_last_indexed_sha(&weave_dir).as_deref(),
        Some("sha1")
    );
}

#[test]
fn try_fast_path_returns_false_when_neither_shortcut_applies() {
    let dir = tempfile::tempdir().unwrap();
    let weave_dir = dir.path().join(".weave");
    fs::create_dir_all(&weave_dir).unwrap();
    let active_db = weave_dir.join("graph.db");

    let took_fast_path =
        try_fast_path(&weave_dir, &active_db, Some("sha1"), None, Instant::now()).unwrap();
    assert!(!took_fast_path);
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
