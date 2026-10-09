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
fn test_cli_init_configures_mcp_and_default_gitignore() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let mut cmd = Command::cargo_bin("weave").unwrap();
    cmd.current_dir(root)
        .arg("init")
        .arg("--mode")
        .arg("single")
        .assert()
        .success();

    let gitignore = std::fs::read_to_string(root.join(".gitignore")).unwrap();
    assert!(gitignore.lines().any(|l| l == ".weave/*"));
    assert!(gitignore.lines().any(|l| l == "!.weave/config.toml"));
    assert!(!gitignore.lines().any(|l| l == ".weave/"));

    let mcp = std::fs::read_to_string(root.join(".mcp.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&mcp).unwrap();
    assert_eq!(
        json["mcpServers"]["weave"]["command"].as_str().unwrap(),
        "weave"
    );
}

#[test]
fn test_cli_init_preserves_existing_mcp_and_updates_ignore() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let initial_mcp = r#"{
  "mcpServers": {
    "graft": {
      "command": "graft",
      "args": ["mcp"]
    }
  }
}"#;
    std::fs::write(root.join(".mcp.json"), initial_mcp).unwrap();
    std::fs::write(root.join(".ignore"), "build/\n").unwrap();

    let mut cmd = Command::cargo_bin("weave").unwrap();
    cmd.current_dir(root)
        .arg("init")
        .arg("--mode")
        .arg("single")
        .assert()
        .success();

    let ignore = std::fs::read_to_string(root.join(".ignore")).unwrap();
    assert!(ignore.lines().any(|l| l == "!.weave/"));
    assert!(ignore.lines().any(|l| l == ".weave/*"));
    assert!(ignore.lines().any(|l| l == "!.weave/config.toml"));
    assert!(!root.join(".gitignore").exists());

    let mcp = std::fs::read_to_string(root.join(".mcp.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&mcp).unwrap();
    assert!(json["mcpServers"]["graft"].is_object());
    assert!(json["mcpServers"]["weave"].is_object());
}

#[test]
fn test_cli_init_allows_git_tracking_of_config_toml() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let init_res = std::process::Command::new("git")
        .args(["init"])
        .current_dir(root)
        .output();
    if init_res.is_err() || !init_res.as_ref().unwrap().status.success() {
        return;
    }

    let mut cmd = Command::cargo_bin("weave").unwrap();
    cmd.current_dir(root)
        .arg("init")
        .arg("--mode")
        .arg("single")
        .assert()
        .success();

    std::fs::write(root.join(".weave/graph.db"), "binary db").unwrap();
    std::fs::write(root.join(".weave/index.lock"), "lock").unwrap();

    let check_cfg = std::process::Command::new("git")
        .args(["check-ignore", ".weave/config.toml"])
        .current_dir(root)
        .output()
        .unwrap();
    assert_eq!(check_cfg.status.code(), Some(1));

    let check_db = std::process::Command::new("git")
        .args(["check-ignore", ".weave/graph.db"])
        .current_dir(root)
        .output()
        .unwrap();
    assert_eq!(check_db.status.code(), Some(0));
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

/// Regression test for a real bug: `weave-graph-cli`'s own `vector`/
/// `policy-lint` Cargo features didn't forward to `weave-graph-mcp`'s
/// matching features, so a `weave` binary built with `--features
/// vector,policy-lint` (or `custom`) compiled fine but never actually
/// registered `weave_search_semantic`/`weave_policy_lint` — invisible to
/// crate-level tests (they build `weave-graph-mcp` directly with its own
/// features set), only catchable through the real compiled binary.
#[test]
#[cfg(all(feature = "vector", feature = "policy-lint"))]
fn test_cli_serve_mcp_advertises_vector_and_policy_lint_tools_when_compiled_in() {
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

    let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n";
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["serve", "--mcp"])
        .write_stdin(input)
        .assert()
        .success()
        .stdout(predicate::str::contains("weave_search_semantic"))
        .stdout(predicate::str::contains("weave_policy_lint"));
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

/// SEC-05 (Phase 4 item 8): `--require-as` refuses to start the server at
/// all without `--as <subject>` — the narrower fix that doesn't touch
/// every other RBAC-gated command's existing "no `--as` == unmasked"
/// default (see `docs/phase3_issues.md`'s own SEC-05 write-up for why the
/// literal "always construct a guard" remediation was rejected).
#[test]
#[cfg(feature = "rbac")]
fn test_cli_serve_mcp_require_as_flag_refuses_to_start_without_as() {
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
        .args(["serve", "--mcp", "--require-as"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("SEC-05"));
}

/// Same gate, reached through `.weave/config.toml`'s `[rbac]
/// require_identity` instead of the `--require-as` flag — the config-driven
/// path a shared/CI deployment would actually set once, not per-invocation.
#[test]
#[cfg(feature = "rbac")]
fn test_cli_serve_mcp_require_identity_config_refuses_to_start_without_as() {
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
    std::fs::write(
        root.join(".weave/config.toml"),
        "mode = \"single\"\n\n[rbac]\nrequire_identity = true\n",
    )
    .unwrap();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["serve", "--mcp"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("SEC-05"));
}

/// The gate must not block a real, authenticated session — `--require-as`
/// plus `--as <subject>` still serves normally.
#[test]
#[cfg(feature = "rbac")]
fn test_cli_serve_mcp_require_as_flag_succeeds_with_as_given() {
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

    let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n";
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["serve", "--mcp", "--require-as", "--as", "alice"])
        .write_stdin(input)
        .assert()
        .success()
        .stdout(predicate::str::contains("weave"));
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

/// `weave report --html` renders standalone offline HTML viewer bundles
/// next to the canvases; `weave viz` re-renders and prints the viewer
/// path without launching a browser (`--open=false`).
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

/// `weave init --mode multiple` emits a CI-cache snippet (exact-sha key +
/// prefix-fallback restore-keys), prints it, and writes it to
/// `.weave/ci-cache.yml`; single mode stays cache-free.
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

#[test]
fn test_cli_init_rejects_an_unrecognized_mode() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["init", "--mode", "custom"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value 'custom'"));

    assert!(!root.join(".weave/config.toml").exists());
}

/// A scripted two-run CI simulation over the snippet the real binary
/// generated — run 1 saves under its exact sha, run 2 misses the sha but
/// takes the restore path via restore-keys.
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

    // Report generates real LOD canvases + a markdown summary
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
    // `ask`/`slm`/`journal` are real commands once `slm` is compiled.
    #[cfg(not(feature = "slm"))]
    uncompiled_commands.push(vec!["ask", "test"]);
    #[cfg(not(feature = "slm"))]
    uncompiled_commands.push(vec!["slm", "list"]);
    #[cfg(not(feature = "slm"))]
    uncompiled_commands.push(vec!["journal"]);
    // `sync pull`/`push` are real commands once `hub` is compiled.
    #[cfg(not(feature = "hub"))]
    uncompiled_commands.push(vec!["sync", "pull"]);
    // `weave index --watch` is real once `watch` is compiled.
    #[cfg(not(feature = "watch"))]
    uncompiled_commands.push(vec!["index", "--watch"]);
    // `weave note ...` is real once `notes` is compiled.
    #[cfg(not(feature = "notes"))]
    uncompiled_commands.push(vec!["note", "list"]);
    // `weave traces ...` and `weave policy ...` are real commands once
    // `otel`/`policy-lint` are compiled.
    #[cfg(not(feature = "otel"))]
    uncompiled_commands.push(vec!["traces", "import", "traces.json"]);
    #[cfg(not(feature = "policy-lint"))]
    uncompiled_commands.push(vec!["policy", "lint"]);
    // `weave rbac serve-scim` and `weave plan-migration` are real once
    // `rbac`/`federation` are compiled.
    #[cfg(not(feature = "rbac"))]
    uncompiled_commands.push(vec!["rbac", "serve-scim"]);
    #[cfg(not(feature = "federation"))]
    uncompiled_commands.push(vec!["plan-migration", "f"]);
    #[cfg(not(feature = "federation"))]
    uncompiled_commands.push(vec!["query-federated", "a", "b", "impact(x)"]);
    #[cfg(not(feature = "federation"))]
    uncompiled_commands.push(vec!["report-federated", "a", "b"]);

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

/// Driven through the compiled binary end to end: two real linked repos,
/// `check-contracts` passes while identical, then fails under `strict`
/// once one repo's exported contract changes.
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
    // `linked_repos` to config — write it directly so `check-contracts`
    // has a repo list to read.
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

/// `weave check-contracts --submodules`: the real-submodule dispatch arm
/// (distinct from the `linked_repos` path above), driven through the
/// compiled binary end to end.
#[test]
#[cfg(feature = "federation")]
fn test_cli_check_contracts_submodules_flag_reports_a_clean_submodule() {
    let inner = tempdir().unwrap();
    let git = |root: &std::path::Path, args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap()
            .success()
    };
    assert!(git(inner.path(), &["init", "-q"]));
    assert!(git(inner.path(), &["config", "user.email", "t@t"]));
    assert!(git(inner.path(), &["config", "user.name", "t"]));
    std::fs::write(inner.path().join("a.rs"), "pub fn a() {}\n").unwrap();
    assert!(git(inner.path(), &["add", "-A"]));
    assert!(git(inner.path(), &["commit", "-q", "-m", "inner"]));

    let outer = tempdir().unwrap();
    let root = outer.path();
    assert!(git(root, &["init", "-q"]));
    assert!(git(root, &["config", "user.email", "t@t"]));
    assert!(git(root, &["config", "user.name", "t"]));
    assert!(git(
        root,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            inner.path().to_str().unwrap(),
            "sub",
        ],
    ));
    assert!(git(root, &["add", "-A"]));
    assert!(git(root, &["commit", "-q", "-m", "add submodule"]));

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

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["check-contracts", "--submodules"])
        .assert()
        .success()
        .stdout(predicate::str::contains("clean"));
}

/// `weave verify`: a clean repo passes in both the default text format and
/// `--format json`, driven through the compiled binary end to end.
#[test]
#[cfg(feature = "federation")]
fn test_cli_verify_passes_on_a_clean_repo_in_text_and_json_format() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("a.rs"), "pub fn a() {}\nfn b() { a(); }\n").unwrap();

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

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("verify")
        .assert()
        .success()
        .stdout(predicate::str::contains("pass"));

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["verify", "--format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"pass\""));
}

/// `--range` without `--file` is refused before any graph lookup — the
/// clear, documented error, not a generic failure.
#[test]
#[cfg(feature = "federation")]
fn test_cli_verify_range_without_file_is_a_clear_error() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("a.rs"), "pub fn a() {}\n").unwrap();
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

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["verify", "--range", "1:10"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--range requires --file"));
}

/// A malformed `--range` (not `start:end` numbers) is a clear parse
/// error, through `main.rs`'s own `parse_range`.
#[test]
#[cfg(feature = "federation")]
fn test_cli_verify_rejects_a_malformed_range() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("a.rs"), "pub fn a() {}\n").unwrap();
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

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["verify", "--file", "a.rs", "--range", "x:5"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not a number"));
}

/// `weave verify` fails with exit code 1 and reports the finding when a
/// phantom symbol is present — the tri-state exit code actually reaches
/// the process, not just the in-process `VerifyReport`.
#[test]
#[cfg(feature = "federation")]
fn test_cli_verify_fails_with_exit_code_one_on_a_phantom_symbol() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("a.rs"),
        "fn caller() { totally_undefined_fn(); }\n",
    )
    .unwrap();
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

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("verify")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("phantom_symbols"));
}

/// `weave pr-review`: a real two-commit diff scored end to end through
/// the compiled binary, not just `pr_review::cmd_pr_review` called
/// in-process.
#[test]
#[cfg(feature = "pr-review")]
fn test_cli_pr_review_scores_a_real_diff_end_to_end() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap()
            .success()
    };
    assert!(git(&["init", "-q", "-b", "main"]));
    assert!(git(&["config", "user.email", "test@example.com"]));
    assert!(git(&["config", "user.name", "Test"]));
    std::fs::write(root.join("core.rs"), "pub fn core() {}\n").unwrap();
    std::fs::write(root.join("feature.rs"), "fn feature() { core(); }\n").unwrap();
    assert!(git(&["add", "-A"]));
    assert!(git(&["commit", "-q", "-m", "base"]));

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

    assert!(git(&["checkout", "-q", "-b", "pr"]));
    std::fs::write(
        root.join("feature.rs"),
        "fn feature() { core(); }\nfn feature2() { feature(); }\n",
    )
    .unwrap();
    assert!(git(&["add", "-A"]));
    assert!(git(&["commit", "-q", "-m", "pr change"]));

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["pr-review", "--base", "main"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Weave PR Review"));
}

/// `weave query-federated`, `weave report-federated`, and `weave
/// plan-migration` against the same linked pair `weave link` already
/// proved out above — the three remaining federation commands `main.rs`
/// dispatches to.
#[test]
#[cfg(feature = "federation")]
fn test_cli_query_federated_report_federated_and_plan_migration_end_to_end() {
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

    Command::cargo_bin("weave")
        .unwrap()
        .args([
            "query-federated",
            &root_a.display().to_string(),
            &root_b.display().to_string(),
            "callers(exported)",
        ])
        .assert()
        .success();

    Command::cargo_bin("weave")
        .unwrap()
        .args([
            "report-federated",
            &root_a.display().to_string(),
            &root_b.display().to_string(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Wrote"));

    std::fs::write(
        root_a.join(".weave").join("config.toml"),
        format!(
            "mode = \"single\"\n\n[federation]\nlinked_repos = [\"{}\"]\n",
            root_b.display()
        ),
    )
    .unwrap();

    // These two repos share no actual cross-repo call edge (just the same
    // function name in each), so there's genuinely nothing to migrate —
    // still a real, exercised `cmd_plan_migration` code path, not a stub.
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root_a)
        .args(["plan-migration", "exported"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no cross-repo callers"));
}

/// `weave sync pull` without a configured `[hub] url` fails clearly,
/// before any network attempt — the real dispatch arm, not just
/// `sync::cmd_sync_pull` called directly.
#[test]
#[cfg(feature = "hub")]
fn test_cli_sync_pull_requires_a_configured_hub_url() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("a.rs"), "pub fn a() {}\n").unwrap();
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

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["sync", "pull"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("[hub] url"));
}

/// `weave note pin` / `weave note list`: real round trip through the
/// compiled binary.
#[test]
#[cfg(feature = "notes")]
fn test_cli_note_pin_and_list_round_trip() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("a.rs"), "pub fn a() {}\n").unwrap();
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

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["note", "pin", "a", "worth remembering"])
        .assert()
        .success();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["note", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("worth remembering"));
}

/// `weave rbac serve-scim`: binds loopback and answers a real SCIM
/// request — through the compiled binary's own dispatch arm, not just
/// `ScimServer` driven in-process.
#[test]
#[cfg(feature = "rbac")]
fn test_cli_rbac_serve_scim_starts_a_real_loopback_server() {
    use std::io::{Read, Write};

    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".weave")).unwrap();

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_weave"))
        .current_dir(root)
        .args(["rbac", "serve-scim", "--port", "0"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    // The server prints its bound address before blocking in the accept
    // loop — read that one line to learn the real (OS-assigned) port.
    let mut reader = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    std::io::BufRead::read_line(&mut reader, &mut line).unwrap();
    let addr = line
        .split("on ")
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .unwrap_or_default()
        .trim()
        .to_string();
    assert!(!addr.is_empty(), "could not parse bound address: {line:?}");

    let mut stream = std::net::TcpStream::connect(&addr).unwrap();
    stream.write_all(b"GET /Users HTTP/1.0\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.0 200"), "{response}");

    let _ = child.kill();
    let _ = child.wait();
}

/// `weave index --watch` picks up a real file change on its own, through
/// a real spawned process and a real (fast-debounced) watcher — not a
/// mocked filesystem event.
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

/// A change whose blast radius meets/exceeds `[watch] blast_radius_ceiling`
/// defers behind the visible `pending-manual-reindex` marker instead of
/// auto-reindexing, and a manual `weave index` clears it and picks up the
/// change for real.
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

/// A real `weave serve --mcp` process with `[watch] enabled` running in
/// the background surfaces the in-flight (still-inside-the-debounce-window)
/// staleness marker in a real `tools/call` response — not just
/// `weave status`.
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

/// One test asserting CLI `query`, `report`, `.canvas` `export`, and an
/// MCP tool call from four different simulated identities all return
/// consistently masked results for the same underlying graph — proving
/// one guard, not four that could drift. `secret_helper` is a private fn
/// only `public_entry` calls; `"internal"` is the one role that bypasses
/// masking.
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

/// Through the real binary: a policy declaring a boundary the repo
/// violates blocks (non-zero exit, the CI gate), and the same repo under
/// a compliant policy does not.
#[test]
#[cfg(feature = "policy-lint")]
fn test_cli_policy_lint_blocks_violation_and_passes_compliant_repo() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let ui = root.join("src/ui");
    let db = root.join("src/db");
    std::fs::create_dir_all(&ui).unwrap();
    std::fs::create_dir_all(&db).unwrap();
    std::fs::write(
        ui.join("view.rs"),
        "fn render() { crate::db::store::save(); }",
    )
    .unwrap();
    std::fs::write(db.join("store.rs"), "pub fn save() {}").unwrap();

    let mut init = Command::cargo_bin("weave").unwrap();
    init.current_dir(root).arg("init").assert().success();
    let mut index = Command::cargo_bin("weave").unwrap();
    index.current_dir(root).arg("index").assert().success();

    let weave_dir = root.join(".weave");
    let policy = weave_dir.join("policy.yaml");
    std::fs::write(
        &policy,
        "rules:\n  - disallow:\n      from: src/ui\n      to: src/db\n",
    )
    .unwrap();

    // Violating repo: blocked, with the violation spelled out.
    let mut lint = Command::cargo_bin("weave").unwrap();
    lint.current_dir(root)
        .args(["policy", "lint"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("disallow"))
        .stderr(predicate::str::contains("policy violation"));

    // Same repo, a boundary it actually respects: the gate passes.
    std::fs::write(
        &policy,
        "rules:\n  - disallow:\n      from: src/db\n      to: src/ui\n",
    )
    .unwrap();
    let mut lint_ok = Command::cargo_bin("weave").unwrap();
    lint_ok
        .current_dir(root)
        .args(["policy", "lint"])
        .assert()
        .success()
        .stdout(predicate::str::contains("no boundary violations"));
}

/// Drift analytics through the real binary: a synthetic two-file cycle
/// and an isolated orphan are both reported.
#[test]
#[cfg(feature = "policy-lint")]
fn test_cli_policy_drift_reports_cycles_and_orphans() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("a.rs"), "fn ping() { crate::pong(); }").unwrap();
    std::fs::write(root.join("b.rs"), "fn pong() { crate::ping(); }").unwrap();
    std::fs::write(root.join("c.rs"), "fn lonely() {}").unwrap();

    let mut init = Command::cargo_bin("weave").unwrap();
    init.current_dir(root).arg("init").assert().success();
    let mut index = Command::cargo_bin("weave").unwrap();
    index.current_dir(root).arg("index").assert().success();

    let mut drift = Command::cargo_bin("weave").unwrap();
    drift
        .current_dir(root)
        .args(["policy", "drift"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("dependency cycle(s)")
                .and(predicate::str::contains("orphaned file(s)")),
        );
}

/// POL-03 through the real binary: a public utility whose only caller is
/// a private (masked) function must be flagged as a masking artifact, not
/// reported as a plain orphan indistinguishable from a real one.
#[test]
#[cfg(all(feature = "policy-lint", feature = "rbac"))]
fn test_cli_policy_drift_annotates_orphans_hidden_by_masking() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("caller.rs"),
        "fn hidden_caller() { crate::exposed::exposed_util(); }",
    )
    .unwrap();
    std::fs::write(root.join("exposed.rs"), "pub fn exposed_util() {}").unwrap();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("init")
        .assert()
        .success();
    std::fs::write(root.join(".weave/config.toml"), "[rbac.users]\nbob = []\n").unwrap();
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("index")
        .assert()
        .success();

    // Unmasked: the real inbound edge is visible, so `exposed.rs` is not
    // orphaned at all (`caller.rs` legitimately is — nothing calls it).
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["policy", "drift"])
        .assert()
        .success()
        .stdout(predicate::str::contains("exposed.rs").not());

    // Masked as `bob`: the caller is hidden, severing the only inbound
    // edge — must be labeled a masking artifact, not a bare orphan.
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["--as", "bob", "policy", "drift"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "exposed.rs (has hidden inbound edges)",
        ));
}

/// Through the real binary: an OTLP trace export annotates the matching
/// symbol, its latency is queryable via `weave query`, and it survives a
/// full reindex (the carry-over path).
#[test]
#[cfg(feature = "otel")]
fn test_cli_traces_import_annotates_node_and_latency_is_queryable() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("lib.rs"), "fn handle() {}").unwrap();

    let mut init = Command::cargo_bin("weave").unwrap();
    init.current_dir(root).arg("init").assert().success();
    let mut index = Command::cargo_bin("weave").unwrap();
    index.current_dir(root).arg("index").assert().success();

    std::fs::write(
        root.join("traces.json"),
        r#"{"resourceSpans":[{"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"api"}}]},"scopeSpans":[{"spans":[{"traceId":"aaaa","spanId":"s1","name":"handle","startTimeUnixNano":"1000000000","endTimeUnixNano":"1004000000","status":{"statusCode":"STATUS_CODE_OK"},"attributes":[{"key":"code.function","value":{"stringValue":"handle"}}]}]}]}]}"#,
    )
    .unwrap();

    let mut import = Command::cargo_bin("weave").unwrap();
    import
        .current_dir(root)
        .args(["traces", "import", "traces.json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1 matched to graph symbols"));

    let mut latency = Command::cargo_bin("weave").unwrap();
    latency
        .current_dir(root)
        .args(["query", "latency(handle)"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1 span(s)").and(predicate::str::contains("p50 400")));

    // A full reindex writes a fresh database; the span must ride along.
    let mut reindex = Command::cargo_bin("weave").unwrap();
    reindex.current_dir(root).arg("index").assert().success();
    let mut latency_after = Command::cargo_bin("weave").unwrap();
    latency_after
        .current_dir(root)
        .args(["query", "latency(handle)"])
        .assert()
        .success()
        .stdout(predicate::str::contains("p50 400"));
}

#[test]
fn test_cli_blast_reports_a_pr_range_end_to_end() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .unwrap();
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "user.name", "T"]);
    std::fs::write(root.join("lib.rs"), "fn base_fn() {}\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "base"]);
    let base_sha = String::from_utf8(
        std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let base_sha = base_sha.trim().to_string();

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

    std::fs::write(root.join("lib.rs"), "fn base_fn() {}\nfn new_fn() {}\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "pr"]);

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["blast", "--base", &base_sha])
        .assert()
        .success()
        .stdout(predicate::str::contains("Weave blast radius"));
}

#[test]
fn test_cli_incremental_index_and_fast_path_over_git() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .unwrap();
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "user.name", "T"]);
    std::fs::write(root.join("lib.rs"), "fn first() {}\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "first"]);

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

    // Modify + commit, then an incremental index picks up only the delta.
    std::fs::write(root.join("lib.rs"), "fn first() {}\nfn second() {}\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "second"]);
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .args(["index", "--incremental"])
        .assert()
        .success();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Total Symbols:  2"));

    // Re-indexing the same commit takes the fast path.
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("index")
        .assert()
        .success()
        .stdout(predicate::str::contains("Already up to date"));
}

#[test]
fn test_cli_index_rebuilds_when_extractor_version_is_stale() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .unwrap();
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "user.name", "T"]);
    std::fs::write(root.join("lib.rs"), "fn first() {}\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "first"]);

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

    // Simulate an index built by an older `weave` release: no file or commit
    // changed, so without the extractor-version check this would silently
    // hit the "Already up to date" fast path and never re-run the (in this
    // simulation, fixed) extraction logic.
    std::fs::write(root.join(".weave/extractor_version"), "0").unwrap();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("index")
        .assert()
        .success()
        .stdout(predicate::str::contains("Already up to date").not())
        .stdout(predicate::str::contains("Indexed"));

    // The rebuild stamps the current version, so a third run on the same
    // commit takes the fast path again instead of rebuilding every time.
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("index")
        .assert()
        .success()
        .stdout(predicate::str::contains("Already up to date"));
}

#[test]
fn test_cli_index_clears_the_snapshot_cache_on_extractor_version_mismatch() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .unwrap();
    };
    let git_sha = || {
        String::from_utf8(
            std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(root)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string()
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "user.name", "T"]);
    std::fs::write(root.join("lib.rs"), "fn first() {}\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "first"]);
    let sha_a = git_sha();

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("init")
        .assert()
        .success();
    // Caches a snapshot for commit A.
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("index")
        .assert()
        .success();

    std::fs::write(root.join("lib.rs"), "fn first() {}\nfn second() {}\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "second"]);
    let sha_b = git_sha();
    // Caches a (soon-to-be-stale) snapshot for commit B too.
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("index")
        .assert()
        .success();

    // Simulate an upgrade: the stamp now disagrees with the running binary.
    std::fs::write(root.join(".weave/extractor_version"), "0").unwrap();
    git(&["checkout", "-q", &sha_a]);
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("index")
        .assert()
        .success()
        .stdout(predicate::str::contains("Indexed"));

    // Without clearing the cache above, B's pre-upgrade snapshot would still
    // be sitting on disk here and get silently restored instead of reindexed.
    git(&["checkout", "-q", &sha_b]);
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("index")
        .assert()
        .success()
        .stdout(predicate::str::contains("Restored cached index").not())
        .stdout(predicate::str::contains("Indexed"));
}

#[test]
fn test_cli_index_without_init_creates_the_weave_dir() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("main.rs"), "fn hello() {}").unwrap();

    // No `weave init` first — `weave index` creates `.weave/` itself.
    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("index")
        .assert()
        .success();
    assert!(root.join(".weave/graph.db").exists());

    Command::cargo_bin("weave")
        .unwrap()
        .current_dir(root)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Total Symbols:"));
}
