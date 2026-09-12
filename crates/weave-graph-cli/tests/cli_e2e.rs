use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

#[test]
fn test_cli_help() {
    let mut cmd = Command::cargo_bin("weave").unwrap();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Ultra-lightweight code intelligence engine",
        ));
}

#[test]
fn test_cli_init_and_index() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    std::fs::write(root.join("main.rs"), "fn main() { println!(\"hello\"); }").unwrap();

    let mut cmd_init = Command::cargo_bin("weave").unwrap();
    cmd_init
        .current_dir(root)
        .arg("init")
        .arg("--mode")
        .arg("single")
        .assert()
        .success();

    let mut cmd_index = Command::cargo_bin("weave").unwrap();
    cmd_index
        .current_dir(root)
        .arg("index")
        .assert()
        .success()
        .stdout(predicate::str::contains("Indexed"));

    let mut cmd_status = Command::cargo_bin("weave").unwrap();
    cmd_status
        .current_dir(root)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Total Symbols:"));
}

#[test]
fn test_cli_index_multi_language_fixtures() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let fixtures_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("weave-graph-parse/tests/fixtures");

    // Copy Rust, TS, and Python sample files
    for file in &["sample.rs", "sample.ts", "sample.py"] {
        let content = std::fs::read(fixtures_dir.join(file)).unwrap();
        std::fs::write(root.join(file), content).unwrap();
    }

    let mut cmd_init = Command::cargo_bin("weave").unwrap();
    cmd_init.current_dir(root).arg("init").assert().success();

    let mut cmd_index = Command::cargo_bin("weave").unwrap();
    cmd_index
        .current_dir(root)
        .arg("index")
        .assert()
        .success()
        .stdout(predicate::str::contains("Indexed"));

    let mut cmd_status = Command::cargo_bin("weave").unwrap();
    cmd_status
        .current_dir(root)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Total Symbols:"));
}

#[test]
fn test_cli_serve_mcp_stdio() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("main.rs"), "fn hello() {}").unwrap();

    let mut cmd_init = Command::cargo_bin("weave").unwrap();
    cmd_init.current_dir(root).arg("init").assert().success();

    let mut cmd_index = Command::cargo_bin("weave").unwrap();
    cmd_index.current_dir(root).arg("index").assert().success();

    let mut cmd_serve = Command::cargo_bin("weave").unwrap();
    let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n";
    cmd_serve
        .current_dir(root)
        .arg("serve")
        .arg("--mcp")
        .write_stdin(input)
        .assert()
        .success()
        .stdout(predicate::str::contains("weave"))
        .stdout(predicate::str::contains("weave_repo_map"))
        .stdout(predicate::str::contains("weave_file_api"));
}

#[test]
fn test_cli_serve_mcp_rejects_non_loopback_without_flag() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("main.rs"), "fn hello() {}").unwrap();

    let mut cmd_init = Command::cargo_bin("weave").unwrap();
    cmd_init.current_dir(root).arg("init").assert().success();

    let mut cmd_index = Command::cargo_bin("weave").unwrap();
    cmd_index.current_dir(root).arg("index").assert().success();

    let mut cmd_serve = Command::cargo_bin("weave").unwrap();
    cmd_serve
        .current_dir(root)
        .arg("serve")
        .arg("--mcp")
        .arg("--transport")
        .arg("http")
        .arg("--host")
        .arg("0.0.0.0")
        .assert()
        .failure();
}

#[test]
fn test_cli_serve_without_mcp_flag_fails() {
    let mut cmd = Command::cargo_bin("weave").unwrap();
    cmd.arg("serve").assert().failure();
}

#[test]
fn test_cli_serve_mcp_fails_clearly_when_no_index_exists() {
    let dir = tempdir().unwrap();
    let mut cmd = Command::cargo_bin("weave").unwrap();
    cmd.current_dir(dir.path())
        .args(["serve", "--mcp"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("weave init && weave index"));
}

#[test]
fn test_cli_serve_mcp_rejects_an_unknown_transport() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("main.rs"), "fn hello() {}").unwrap();
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("init")
        .assert()
        .success();
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("index")
        .assert()
        .success();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["serve", "--mcp", "--transport", "bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown transport"));
}

#[test]
fn test_cli_config_set_creates_the_weave_dir_when_missing() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    // No `weave init` first — `.weave/` doesn't exist yet.
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["config", "set", "mode", "single"])
        .assert()
        .success();
    assert!(root.join(".weave").join("config.toml").exists());
}

#[test]
fn test_cli_query_reports_a_clear_error_for_an_unknown_symbol() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("main.rs"), "fn hello() {}").unwrap();
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("init")
        .assert()
        .success();
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("index")
        .assert()
        .success();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["query", "callers(does_not_exist)"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("symbol not found"));
}

#[test]
fn test_cli_export_reports_a_clear_error_for_an_unknown_symbol() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("main.rs"), "fn hello() {}").unwrap();
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("init")
        .assert()
        .success();
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("index")
        .assert()
        .success();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["export", "--symbol", "does_not_exist"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("symbol not found"));
}

#[test]
fn test_cli_config_set_and_get_storage_home() {
    let repo_dir = tempdir().unwrap();
    let storage_dir = tempdir().unwrap();
    let root = repo_dir.path();
    let external_home = storage_dir.path().to_str().unwrap();

    std::fs::write(root.join("main.rs"), "fn hello() {}").unwrap();

    let mut cmd_init = Command::cargo_bin("weave").unwrap();
    cmd_init.current_dir(root).arg("init").assert().success();

    let mut cmd_set = Command::cargo_bin("weave").unwrap();
    cmd_set
        .current_dir(root)
        .arg("config")
        .arg("set")
        .arg("storage.home")
        .arg(external_home)
        .assert()
        .success();

    let mut cmd_get = Command::cargo_bin("weave").unwrap();
    cmd_get
        .current_dir(root)
        .arg("config")
        .arg("get")
        .arg("storage.home")
        .assert()
        .success()
        .stdout(predicate::str::contains(external_home));

    let mut cmd_index = Command::cargo_bin("weave").unwrap();
    cmd_index.current_dir(root).arg("index").assert().success();

    // Verify graph.db exists in external storage folder, NOT in local .weave/
    assert!(storage_dir.path().join("graph.db").exists());
    assert!(!root.join(".weave/graph.db").exists());

    let mut cmd_status = Command::cargo_bin("weave").unwrap();
    cmd_status
        .current_dir(root)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Total Symbols:"));
}

#[test]
fn test_cli_config_get_missing_key_fails() {
    let repo_dir = tempdir().unwrap();
    let root = repo_dir.path();

    let mut cmd_init = Command::cargo_bin("weave").unwrap();
    cmd_init.current_dir(root).arg("init").assert().success();

    let mut cmd_get = Command::cargo_bin("weave").unwrap();
    cmd_get
        .current_dir(root)
        .arg("config")
        .arg("get")
        .arg("storage.home")
        .assert()
        .failure();
}

#[test]
fn test_cli_init_multiple_and_existing_config() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let mut cmd_init = Command::cargo_bin("weave").unwrap();
    cmd_init
        .current_dir(root)
        .arg("init")
        .arg("--mode")
        .arg("multiple")
        .assert()
        .success();

    let mut cmd_get = Command::cargo_bin("weave").unwrap();
    cmd_get
        .current_dir(root)
        .arg("config")
        .arg("get")
        .arg("mode")
        .assert()
        .success()
        .stdout(predicate::str::contains("multiple"));

    // Running init again prints existing config found
    let mut cmd_init2 = Command::cargo_bin("weave").unwrap();
    cmd_init2
        .current_dir(root)
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Existing .weave/config.toml found",
        ));
}

/// impl.md M2.13: `weave report --html` renders standalone offline HTML
/// viewer bundles next to the canvases; `weave viz` re-renders and prints
/// the viewer path without launching a browser (`--open=false`).
#[test]
#[cfg(feature = "viz")]
fn test_cli_report_html_and_viz_command() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("main.rs"), "fn a() { b(); }\nfn b() {}").unwrap();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["init"])
        .assert()
        .success();
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("index")
        .assert()
        .success();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["report", "--html"])
        .assert()
        .success()
        .stdout(predicate::str::contains("weave-report.html"));

    let report_html = root.join(".weave/report/weave-report.html");
    let html = std::fs::read_to_string(&report_html).unwrap();
    assert!(html.contains("<svg"));
    assert!(html.contains("\"nodes\""));
    assert!(root.join(".weave/report/weave-modules.html").exists());

    // `weave viz` re-renders from the existing report without launching
    // a browser (--open=false).
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["viz", "--open=false"])
        .assert()
        .success()
        .stdout(predicate::str::contains("weave-report.html"));

    // Before any report, viz is a clear error, not a panic.
    let dir2 = tempdir().unwrap();
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(dir2.path())
        .args(["viz", "--open=false"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("weave report"));
}

/// impl.md M2.6: `weave init --mode multiple` emits the L1 CI-cache snippet
/// (exact-sha key + prefix-fallback restore-keys), prints it, and writes it
/// to `.weave/ci-cache.yml`; single mode stays cache-free (plan.md §1.3).
#[test]
fn test_cli_init_multiple_emits_ci_cache_snippet() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["init", "--mode", "multiple"])
        .assert()
        .success()
        .stdout(predicate::str::contains("actions/cache"));

    let snippet = std::fs::read_to_string(root.join(".weave/ci-cache.yml")).unwrap();
    assert!(
        snippet.contains("key: weave-${{ runner.os }}-${{ github.ref_name }}-${{ github.sha }}")
    );
    assert!(snippet.contains("weave-${{ runner.os }}-${{ github.ref_name }}-"));
    assert!(snippet.contains("weave-${{ runner.os }}-main-"));
    assert!(snippet.contains("fetch-depth: 0"));
    assert!(snippet.contains("zstd"));
}

#[test]
fn test_cli_init_single_does_not_emit_ci_cache_snippet() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["init", "--mode", "single"])
        .assert()
        .success();

    assert!(!root.join(".weave/ci-cache.yml").exists());
}

/// impl.md M2.6 acceptance: a scripted two-run CI simulation over the
/// snippet the real binary generated — run 1 saves under its exact sha,
/// run 2 misses the sha but takes the restore path via restore-keys.
#[test]
fn test_cli_init_multiple_snippet_restores_cache_on_second_run() {
    fn cache_restore(
        caches: &[(String, String)],
        key: &str,
        restore_keys: &[&str],
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

    let dir = tempdir().unwrap();
    let root = dir.path();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["init", "--mode", "multiple"])
        .assert()
        .success();

    let snippet = std::fs::read_to_string(root.join(".weave/ci-cache.yml")).unwrap();
    let key = snippet
        .lines()
        .map(str::trim_start)
        .find(|l| l.starts_with("key:"))
        .unwrap()
        .trim_start_matches("key:")
        .trim()
        .to_string();
    let restore_keys = ["weave-Linux-feature-", "weave-Linux-main-"];

    // Run 1: cold — no exact match, no prefix match, nothing to restore.
    let mut caches: Vec<(String, String)> = Vec::new();
    let run1_key = key
        .replace("${{ runner.os }}", "Linux")
        .replace("${{ github.ref_name }}", "feature")
        .replace("${{ github.sha }}", "aaa");
    assert!(cache_restore(&caches, &run1_key, &restore_keys).is_none());
    caches.push((run1_key, "graph-aaa".to_string()));

    // Run 2: exact sha miss, prefix fallback restores run 1's cache.
    let run2_key = key
        .replace("${{ runner.os }}", "Linux")
        .replace("${{ github.ref_name }}", "feature")
        .replace("${{ github.sha }}", "bbb");
    assert_eq!(
        cache_restore(&caches, &run2_key, &restore_keys).as_deref(),
        Some("graph-aaa"),
        "second run must take the restore path, not cold-index"
    );
}

#[test]
fn test_cli_query_export_report_and_reindex_fast_path() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    std::fs::write(
        root.join("main.rs"),
        "fn caller() { callee(); }\nfn callee() {}",
    )
    .unwrap();

    let mut cmd_init = Command::cargo_bin("weave").unwrap();
    cmd_init.current_dir(root).arg("init").assert().success();

    let mut cmd_index = Command::cargo_bin("weave").unwrap();
    cmd_index.current_dir(root).arg("index").assert().success();

    // Query callers
    let mut cmd_query = Command::cargo_bin("weave").unwrap();
    cmd_query
        .current_dir(root)
        .arg("query")
        .arg("callees(caller)")
        .assert()
        .success();

    // Export symbol neighborhood
    let mut cmd_export = Command::cargo_bin("weave").unwrap();
    cmd_export
        .current_dir(root)
        .arg("export")
        .arg("--symbol")
        .arg("caller")
        .arg("--depth")
        .arg("1")
        .assert()
        .success()
        .stdout(predicate::str::contains("caller"));

    // Report generates real LOD canvases + a markdown summary (M1.8)
    let mut cmd_report = Command::cargo_bin("weave").unwrap();
    cmd_report
        .current_dir(root)
        .arg("report")
        .assert()
        .success()
        .stdout(predicate::str::contains("WEAVE_REPORT.md"));
    assert!(root.join(".weave/report/WEAVE_REPORT.md").exists());
    assert!(root.join(".weave/report/weave-report.canvas").exists());
    assert!(root.join(".weave/report/weave-modules.canvas").exists());

    // Fast path: reindex unchanged git working tree or second index
    let mut cmd_index_again = Command::cargo_bin("weave").unwrap();
    cmd_index_again
        .current_dir(root)
        .arg("index")
        .assert()
        .success();
}

#[test]
fn test_cli_empty_repo_and_status_without_index() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let mut cmd_init = Command::cargo_bin("weave").unwrap();
    cmd_init.current_dir(root).arg("init").assert().success();

    // Status before indexing reports no database found
    let mut cmd_status = Command::cargo_bin("weave").unwrap();
    cmd_status
        .current_dir(root)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("No graph database found"));

    // Index on empty repo reports no indexable files found
    let mut cmd_index = Command::cargo_bin("weave").unwrap();
    cmd_index
        .current_dir(root)
        .arg("index")
        .assert()
        .success()
        .stdout(predicate::str::contains("No indexable code"));
}

#[test]
fn test_cli_uncompiled_features_fail_with_clear_message() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Every one of these is a real command once its gating feature is
    // compiled in, so it's only still a stub in a build that leaves that
    // feature out. mut is only exercised when at least one push below runs.
    #[allow(unused_mut)]
    let mut uncompiled_commands: Vec<Vec<&str>> = Vec::new();
    #[cfg(not(feature = "federation"))]
    uncompiled_commands.push(vec!["link", "a", "b"]);
    #[cfg(not(feature = "federation"))]
    uncompiled_commands.push(vec!["check-contracts"]);
    // `ask`/`slm`/`journal` (M2.4) are real commands once `slm` is compiled.
    #[cfg(not(feature = "slm"))]
    uncompiled_commands.push(vec!["ask", "test"]);
    #[cfg(not(feature = "slm"))]
    uncompiled_commands.push(vec!["slm", "list"]);
    #[cfg(not(feature = "slm"))]
    uncompiled_commands.push(vec!["journal"]);
    // `sync pull`/`push` (M2.5) are real commands once `hub` is compiled.
    #[cfg(not(feature = "hub"))]
    uncompiled_commands.push(vec!["sync", "pull"]);
    // `weave index --watch` is real once `watch` is compiled.
    #[cfg(not(feature = "watch"))]
    uncompiled_commands.push(vec!["index", "--watch"]);
    // `weave note ...` (M2.10) is real once `notes` is compiled.
    #[cfg(not(feature = "notes"))]
    uncompiled_commands.push(vec!["note", "list"]);

    for args in uncompiled_commands {
        let mut cmd = Command::cargo_bin("weave").unwrap();
        cmd.current_dir(root)
            .args(&args)
            .assert()
            .failure()
            .stderr(predicate::str::contains("requires the"));
    }
}

/// Once `federation` is compiled, `weave link` is real — it fails on two
/// bare, never-indexed repo paths, but with a graph-database error, not
/// the "requires the ... feature" stub message the test above checks for.
#[test]
#[cfg(feature = "federation")]
fn test_cli_link_runs_for_real_once_federation_is_compiled() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let mut cmd = Command::cargo_bin("weave").unwrap();
    cmd.current_dir(root)
        .args(["link", "a", "b"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No graph database found"));
}

/// impl.md M2.2's Acceptance Criteria Test D, driven through the compiled
/// binary end to end: two real linked repos, `check-contracts` passes while
/// identical, then fails under `strict` once one repo's exported contract
/// changes.
#[test]
#[cfg(feature = "federation")]
fn test_cli_check_contracts_detects_divergence_end_to_end() {
    let dir_a = tempdir().unwrap();
    let dir_b = tempdir().unwrap();
    let root_a = dir_a.path();
    let root_b = dir_b.path();

    std::fs::write(
        root_a.join("lib.rs"),
        "pub fn exported(x: u32) -> u32 { x }\n",
    )
    .unwrap();
    std::fs::write(
        root_b.join("lib.rs"),
        "pub fn exported(x: u32) -> u32 { x }\n",
    )
    .unwrap();

    for root in [root_a, root_b] {
        Command::cargo_bin("weave")
            .unwrap()
            .current_dir(root)
            .args(["init", "--mode", "single"])
            .assert()
            .success();
        Command::cargo_bin("weave")
            .unwrap()
            .current_dir(root)
            .arg("index")
            .assert()
            .success();
    }

    Command::cargo_bin("weave")
        .unwrap()
        .args([
            "link",
            &root_a.display().to_string(),
            &root_b.display().to_string(),
        ])
        .assert()
        .success();

    // `weave link` records contract expectations but doesn't yet append
    // `linked_repos` to config (impl.md M2.1's still-open write-side gap) —
    // write it directly so `check-contracts` has a repo list to read.
    std::fs::write(
        root_a.join(".weave").join("config.toml"),
        format!(
            "mode = \"single\"\n\n[federation]\nlinked_repos = [\"{}\"]\nstaleness_policy = \"strict\"\n",
            root_b.display()
        ),
    )
    .unwrap();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root_a)
        .arg("check-contracts")
        .assert()
        .success()
        .stdout(predicate::str::contains("up to date"));

    // Diverge repo_b's exported contract (param type change).
    std::fs::write(
        root_b.join("lib.rs"),
        "pub fn exported(x: u64) -> u64 { x }\n",
    )
    .unwrap();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root_a)
        .arg("check-contracts")
        .assert()
        .failure()
        .stdout(predicate::str::contains("Contract drift detected"));
}

/// impl.md M2.11: `weave index --watch` picks up a real file change on its
/// own, through a real spawned process and a real (fast-debounced) watcher
/// — not a mocked filesystem event.
#[test]
#[cfg(feature = "watch")]
fn test_cli_index_watch_auto_reindexes_on_file_change() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("lib.rs"), "fn only_fn() {}\n").unwrap();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["init", "--mode", "single"])
        .assert()
        .success();
    // Fast debounce so the test doesn't have to wait on the 2s default.
    std::fs::write(
        root.join(".weave").join("config.toml"),
        "mode = \"single\"\n\n[watch]\ndebounce_ms = 100\n",
    )
    .unwrap();
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("index")
        .assert()
        .success();

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_weave"))
        .current_dir(root)
        .args(["index", "--watch"])
        .spawn()
        .unwrap();

    // Let the watcher actually start before touching anything.
    std::thread::sleep(std::time::Duration::from_millis(400));
    std::fs::write(root.join("lib.rs"), "fn only_fn() {}\nfn added_fn() {}\n").unwrap();
    // Debounce (100ms) + a real incremental reindex + margin.
    std::thread::sleep(std::time::Duration::from_millis(2000));
    let _ = child.kill();
    let _ = child.wait();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Total Symbols:  2"));
}

/// impl.md M2.11: a change whose blast radius meets/exceeds
/// `[watch] blast_radius_ceiling` defers behind the visible
/// `pending-manual-reindex` marker instead of auto-reindexing, and a
/// manual `weave index` clears it and picks up the change for real.
#[test]
#[cfg(feature = "watch")]
fn test_cli_index_watch_defers_a_large_blast_radius_change() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("lib.rs"),
        "fn caller() { callee(); }\nfn callee() { helper(); }\nfn helper() {}\n",
    )
    .unwrap();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["init", "--mode", "single"])
        .assert()
        .success();
    // Ceiling of 1: any change touching this 3-symbol call chain exceeds it.
    std::fs::write(
        root.join(".weave").join("config.toml"),
        "mode = \"single\"\n\n[watch]\ndebounce_ms = 100\nblast_radius_ceiling = 1\n",
    )
    .unwrap();
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("index")
        .assert()
        .success();

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_weave"))
        .current_dir(root)
        .args(["index", "--watch"])
        .spawn()
        .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(400));
    std::fs::write(
        root.join("lib.rs"),
        "fn caller() { callee(); }\nfn callee() { helper(); }\nfn helper() { extra(); }\nfn extra() {}\n",
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2000));
    let _ = child.kill();
    let _ = child.wait();

    // Deferred: the auto-sync never ran, so the symbol count is unchanged
    // and the pending marker is visible in `weave status`.
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Total Symbols:  3"))
        .stdout(predicate::str::contains("blast radius pending"));

    // A manual `weave index` clears the marker and picks up the change.
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("index")
        .assert()
        .success();
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Total Symbols:  4"))
        .stdout(predicate::str::contains("blast radius pending").not());
}

/// impl.md M2.11's last open task, closed: a real `weave serve --mcp`
/// process with `[watch] enabled` running in the background surfaces the
/// in-flight (still-inside-the-debounce-window) staleness marker in a real
/// `tools/call` response — not just `weave status`.
#[test]
#[cfg(feature = "watch")]
fn test_cli_serve_mcp_surfaces_watch_staleness_in_tool_responses() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;

    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("lib.rs"), "fn only_fn() {}\n").unwrap();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["init", "--mode", "single"])
        .assert()
        .success();
    std::fs::write(
        root.join(".weave").join("config.toml"),
        "mode = \"single\"\n\n[watch]\nenabled = true\ndebounce_ms = 300\n",
    )
    .unwrap();
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("index")
        .assert()
        .success();

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_weave"))
        .current_dir(root)
        .args(["serve", "--mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();

    // Let the background watcher thread actually start before editing.
    std::thread::sleep(std::time::Duration::from_millis(400));
    std::fs::write(root.join("lib.rs"), "fn only_fn() {}\nfn added_fn() {}\n").unwrap();
    // Still well inside the 300ms debounce window.
    std::thread::sleep(std::time::Duration::from_millis(50));

    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"weave_repo_map","arguments":{{}}}}}}"#
    )
    .unwrap();
    let response = lines.next().unwrap().unwrap();

    let _ = child.kill();
    let _ = child.wait();

    assert!(response.contains("debounce window"), "got: {response}");
    assert!(response.contains("lib.rs"), "got: {response}");
}

/// M3.0's own required verify criterion (`impl.md`): one test asserting
/// CLI `query`, `report`, `.canvas` `export`, and an MCP tool call from
/// four different simulated identities all return consistently masked
/// results for the same underlying graph — proving one guard, not four
/// that could drift. `secret_helper` is a private fn only `public_entry`
/// calls; `"internal"` is the one role that bypasses masking.
#[test]
#[cfg(feature = "rbac")]
fn test_cli_rbac_masks_consistently_across_query_report_export_and_mcp() {
    const HIDDEN: &str = "<rbac: hidden>";

    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("main.rs"),
        "pub fn public_entry() { secret_helper(); }\nfn secret_helper() {}\n",
    )
    .unwrap();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("init")
        .assert()
        .success();
    // `alice` is internal (unmasked); `bob`/`carol` are configured but
    // non-internal; `dave` isn't configured at all — all three of the
    // latter must resolve to the same masked (anonymous) view.
    std::fs::write(
        root.join(".weave/config.toml"),
        "mode = \"single\"\n\n[rbac.users]\nalice = [\"internal\"]\nbob = []\ncarol = [\"contractor\"]\n",
    )
    .unwrap();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("index")
        .assert()
        .success();

    let identities: [(&str, bool); 4] = [
        ("alice", true),
        ("bob", false),
        ("carol", false),
        ("dave", false),
    ];

    for (subject, sees_private) in identities {
        let query_out = Command::cargo_bin("weave")
            .unwrap()
            .current_dir(root)
            .args(["query", "callees(public_entry)", "--as", subject])
            .output()
            .unwrap();
        assert!(query_out.status.success(), "query failed for {subject}");
        let query_text = String::from_utf8_lossy(&query_out.stdout);
        assert_eq!(
            query_text.contains("secret_helper"),
            sees_private,
            "query for {subject}: {query_text}"
        );
        assert_eq!(
            query_text.contains(HIDDEN),
            !sees_private,
            "query for {subject}: {query_text}"
        );

        let export_out = Command::cargo_bin("weave")
            .unwrap()
            .current_dir(root)
            .args([
                "export",
                "--symbol",
                "public_entry",
                "--depth",
                "1",
                "--as",
                subject,
            ])
            .output()
            .unwrap();
        assert!(export_out.status.success(), "export failed for {subject}");
        let export_text = String::from_utf8_lossy(&export_out.stdout);
        assert_eq!(
            export_text.contains("secret_helper"),
            sees_private,
            "export for {subject}: {export_text}"
        );

        Command::cargo_bin("weave")
            .unwrap()
            .current_dir(root)
            .args(["report", "--as", subject])
            .assert()
            .success();
        let report_md =
            std::fs::read_to_string(root.join(".weave/report/WEAVE_REPORT.md")).unwrap();
        let expected_total = if sees_private {
            "Total symbols: 2"
        } else {
            "Total symbols: 1"
        };
        assert!(
            report_md.contains(expected_total),
            "report for {subject}: {report_md}"
        );

        let mcp_input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n\
             {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"weave_trace_calls\",\"arguments\":{\"symbol\":\"public_entry\",\"depth\":1}}}\n";
        let mcp_out = Command::cargo_bin("weave")
            .unwrap()
            .current_dir(root)
            .args(["serve", "--mcp", "--as", subject])
            .write_stdin(mcp_input)
            .output()
            .unwrap();
        assert!(mcp_out.status.success(), "mcp failed for {subject}");
        let mcp_text = String::from_utf8_lossy(&mcp_out.stdout);
        assert_eq!(
            mcp_text.contains("secret_helper"),
            sees_private,
            "mcp for {subject}: {mcp_text}"
        );
    }
}
