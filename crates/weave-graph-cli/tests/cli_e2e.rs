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

    // The `hub`/`slm` commands have no implementation yet regardless of
    // build — always stubbed. `link` (M2.1) and `check-contracts` (M2.2)
    // are real commands once `federation` is compiled, so they're only
    // still stubs in a build that leaves that feature out.
    // mut is only exercised when `federation` is compiled out (below).
    #[allow(unused_mut)]
    let mut uncompiled_commands = vec![vec!["sync", "pull"]];
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
