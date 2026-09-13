use serde_json::json;
use weave_graph_core::{Node, Storage};
use weave_graph_store_sqlite::SqliteStorage;

use super::*;

fn setup_storage() -> SqliteStorage {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_node(&Node {
            id: 1,
            repo_id: "test".to_string(),
            path: "src/lib.rs".to_string(),
            symbol: "run".to_string(),
            kind: "function".to_string(),
            line_start: 1,
            line_end: 10,
            signature: "fn run()".to_string(),
        })
        .unwrap();
    storage
}

#[test]
fn handle_parse_error() {
    let storage = setup_storage();
    let handler = McpHandler::new(storage).unwrap();
    let res = handler.handle_message("invalid json").unwrap();
    assert!(res.error.is_some());
    assert_eq!(res.error.unwrap().code, -32700);
}

#[test]
fn handle_initialize() {
    let storage = setup_storage();
    let handler = McpHandler::new(storage).unwrap();
    let msg = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    })
    .to_string();

    let res = handler.handle_message(&msg).unwrap();
    assert_eq!(res.id, Some(json!(1)));
    let result = res.result.unwrap();
    assert_eq!(result["serverInfo"]["name"], "weave");
}

#[test]
fn handle_notifications_and_ping() {
    let storage = setup_storage();
    let handler = McpHandler::new(storage).unwrap();

    let notif = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    })
    .to_string();
    assert!(handler.handle_message(&notif).is_none());

    let ping = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "ping"
    })
    .to_string();
    let res = handler.handle_message(&ping).unwrap();
    assert_eq!(res.id, Some(json!(2)));
}

#[test]
fn handle_tools_list() {
    let storage = setup_storage();
    let handler = McpHandler::new(storage).unwrap();
    let msg = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/list"
    })
    .to_string();

    let res = handler.handle_message(&msg).unwrap();
    let tools = res.result.unwrap()["tools"].as_array().unwrap().clone();
    // 4 base tools, +2 with `notes` (weave_pin_note/recall), +1 with
    // `vector` (weave_search_semantic), +1 with `policy-lint`
    // (weave_policy_lint).
    let expected = 4
        + if cfg!(feature = "notes") { 2 } else { 0 }
        + if cfg!(feature = "vector") { 1 } else { 0 }
        + if cfg!(feature = "policy-lint") { 1 } else { 0 };
    assert_eq!(tools.len(), expected);
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"weave_repo_map"));
    assert!(names.contains(&"weave_file_api"));
    assert!(names.contains(&"weave_trace_calls"));
    assert!(names.contains(&"weave_impact_radius"));
    if cfg!(feature = "vector") {
        assert!(names.contains(&"weave_search_semantic"));
    }
    if cfg!(feature = "policy-lint") {
        assert!(names.contains(&"weave_policy_lint"));
    }
}

#[test]
fn handle_tools_call_all_four_tools() {
    let storage = setup_storage();
    let handler = McpHandler::new(storage).unwrap();

    // 1. repo_map
    let req = json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "tools/call",
        "params": {
            "name": "weave_repo_map",
            "arguments": {}
        }
    })
    .to_string();
    let res = handler.handle_message(&req).unwrap();
    let text = res.result.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("src/lib.rs"));

    // 2. file_api
    let req = json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "tools/call",
        "params": {
            "name": "weave_file_api",
            "arguments": { "paths": ["src/lib.rs"] }
        }
    })
    .to_string();
    let res = handler.handle_message(&req).unwrap();
    let text = res.result.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("src/lib.rs:"));
    assert!(text.contains("run [function] L1-10"));

    // 3. trace_calls
    let req = json!({
        "jsonrpc": "2.0",
        "id": 12,
        "method": "tools/call",
        "params": {
            "name": "weave_trace_calls",
            "arguments": { "symbol": "run", "depth": 2 }
        }
    })
    .to_string();
    let res = handler.handle_message(&req).unwrap();
    let text = res.result.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("trace_calls: run"));

    // 4. impact_radius
    let req = json!({
        "jsonrpc": "2.0",
        "id": 13,
        "method": "tools/call",
        "params": {
            "name": "weave_impact_radius",
            "arguments": { "symbol": "run" }
        }
    })
    .to_string();
    let res = handler.handle_message(&req).unwrap();
    let text = res.result.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("impact_radius: run"));
}

#[cfg(feature = "vector")]
#[test]
fn handle_tools_call_search_semantic() {
    let storage = setup_storage();
    let embedder = weave_graph_core::embedding::MockEmbeddingProvider::new();
    storage
        .rebuild_vector_index(&embedder, &[(1, "runs the whole program".to_string())])
        .unwrap();
    let handler = McpHandler::new(storage).unwrap();

    let req = json!({
        "jsonrpc": "2.0",
        "id": 20,
        "method": "tools/call",
        "params": {
            "name": "weave_search_semantic",
            "arguments": { "query": "runs the whole program", "limit": 5 }
        }
    })
    .to_string();
    let res = handler.handle_message(&req).unwrap();
    let text = res.result.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("run"), "{text}");
}

#[cfg(feature = "policy-lint")]
#[test]
fn handle_tools_call_policy_lint() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("policy.yaml"),
        "rules:\n  - disallow:\n      from: src\n      to: nonexistent\n",
    )
    .unwrap();
    let storage = setup_storage();
    let handler = McpHandler::new(storage)
        .unwrap()
        .with_weave_dir(dir.path().to_path_buf());

    let req = json!({
        "jsonrpc": "2.0",
        "id": 21,
        "method": "tools/call",
        "params": {
            "name": "weave_policy_lint",
            "arguments": {}
        }
    })
    .to_string();
    let res = handler.handle_message(&req).unwrap();
    let text = res.result.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(text, "✓ no boundary violations");
}

#[test]
fn handle_unknown_tool_and_unknown_method() {
    let storage = setup_storage();
    let handler = McpHandler::new(storage).unwrap();

    let req = json!({
        "jsonrpc": "2.0",
        "id": 20,
        "method": "tools/call",
        "params": {
            "name": "unknown_tool",
            "arguments": {}
        }
    })
    .to_string();
    let res = handler.handle_message(&req).unwrap();
    assert_eq!(res.result.unwrap()["isError"], true);

    let req2 = json!({
        "jsonrpc": "2.0",
        "id": 21,
        "method": "unknown_method"
    })
    .to_string();
    let res2 = handler.handle_message(&req2).unwrap();
    assert_eq!(res2.error.unwrap().code, -32601);

    let notif = json!({
        "jsonrpc": "2.0",
        "method": "unknown_notification"
    })
    .to_string();
    assert!(handler.handle_message(&notif).is_none());
}

#[test]
fn handle_tools_call_missing_params() {
    let storage = setup_storage();
    let handler = McpHandler::new(storage).unwrap();

    let req = json!({
        "jsonrpc": "2.0",
        "id": 30,
        "method": "tools/call"
    })
    .to_string();
    let res = handler.handle_message(&req).unwrap();
    assert_eq!(res.error.unwrap().code, -32602);

    let req2 = json!({
        "jsonrpc": "2.0",
        "id": 31,
        "method": "tools/call",
        "params": {}
    })
    .to_string();
    let res2 = handler.handle_message(&req2).unwrap();
    assert_eq!(res2.error.unwrap().code, -32602);
}

fn call_repo_map_text(handler: &McpHandler) -> String {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 40,
        "method": "tools/call",
        "params": {"name": "weave_repo_map", "arguments": {}}
    })
    .to_string();
    let res = handler.handle_message(&req).unwrap();
    let content = res.result.unwrap()["content"].clone();
    content
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["text"].as_str().unwrap().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// impl.md M2.11: without `with_weave_dir`, tool responses are byte-for-byte
/// unaffected — the watch feature's staleness surfacing must be opt-in.
#[test]
fn no_weave_dir_means_no_staleness_appended() {
    let storage = setup_storage();
    let handler = McpHandler::new(storage).unwrap();
    assert!(!call_repo_map_text(&handler).contains("blast radius"));
    assert!(!call_repo_map_text(&handler).contains("debounce window"));
}

/// impl.md M2.11: `with_weave_dir` set but no marker files present — still
/// unaffected (the common case: most repos are never mid-watch-cycle).
#[test]
fn weave_dir_set_but_no_marker_means_no_staleness_appended() {
    let storage = setup_storage();
    let dir = tempfile::tempdir().unwrap();
    let handler = McpHandler::new(storage)
        .unwrap()
        .with_weave_dir(dir.path().to_path_buf());
    assert!(!call_repo_map_text(&handler).contains("blast radius"));
    assert!(!call_repo_map_text(&handler).contains("debounce window"));
}

/// impl.md M2.11's actual task: a pending-manual-reindex marker on disk
/// shows up in every tool response's content, not just `weave status`.
#[test]
fn pending_reindex_marker_is_surfaced_in_tool_responses() {
    let storage = setup_storage();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("pending-manual-reindex"),
        r#"{"files":["src/big.rs"],"blast_radius":350}"#,
    )
    .unwrap();
    let handler = McpHandler::new(storage)
        .unwrap()
        .with_weave_dir(dir.path().to_path_buf());
    let text = call_repo_map_text(&handler);
    assert!(text.contains("350 symbols"), "got: {text}");
    assert!(text.contains("src/big.rs"), "got: {text}");
}

/// The debounce-window marker is distinct from the deferred one above —
/// worded differently, and both can appear in the same response.
#[test]
fn in_flight_marker_is_surfaced_in_tool_responses() {
    let storage = setup_storage();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("watch-in-flight"), r#"["src/edited.rs"]"#).unwrap();
    let handler = McpHandler::new(storage)
        .unwrap()
        .with_weave_dir(dir.path().to_path_buf());
    let text = call_repo_map_text(&handler);
    assert!(text.contains("debounce window"), "got: {text}");
    assert!(text.contains("src/edited.rs"), "got: {text}");
}

// ─── impl.md M2.15: live reload on external reindex ─────────────────────────

fn seed_file_db(path: &std::path::Path, symbol: &str) {
    let mut storage = SqliteStorage::open(path).unwrap();
    storage
        .upsert_node(&Node {
            id: 1,
            repo_id: "r".into(),
            path: "a.rs".into(),
            symbol: symbol.into(),
            kind: "function".into(),
            line_start: 1,
            line_end: 3,
            signature: String::new(),
        })
        .unwrap();
}

/// Simulates a second-process `weave index` against the same path: the
/// real CLI writes a fresh `.rebuild` file and atomically renames it over
/// the active path (Core Invariant 2) — exactly this.
fn second_process_index(path: &std::path::Path, extra_symbol: Option<&str>) {
    let extra = extra_symbol;
    let rebuild = path.with_extension("db.rebuild");
    let mut storage = SqliteStorage::open(&rebuild).unwrap();
    storage
        .upsert_node(&Node {
            id: 1,
            repo_id: "r".into(),
            path: "a.rs".into(),
            symbol: "first_sym".into(),
            kind: "function".into(),
            line_start: 1,
            line_end: 3,
            signature: String::new(),
        })
        .unwrap();
    if let Some(extra) = extra {
        storage
            .upsert_node(&Node {
                id: 2,
                repo_id: "r".into(),
                path: "b.rs".into(),
                symbol: extra.into(),
                kind: "function".into(),
                line_start: 1,
                line_end: 3,
                signature: String::new(),
            })
            .unwrap();
    }
    drop(storage);
    std::fs::rename(&rebuild, path).unwrap();
}

#[test]
fn handler_detects_external_reindex_and_returns_fresh_data() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("graph.db");
    seed_file_db(&db, "first_sym");

    let handler = McpHandler::open(&db)
        .unwrap()
        .with_recheck_interval(Duration::ZERO);
    let before = call_repo_map_text(&handler);
    assert!(before.contains("a.rs"));
    assert!(!before.contains("b.rs"));

    // A different process reindexes; the open Connection is pinned to the
    // old inode, so the handler must fully close and reopen.
    second_process_index(&db, Some("brand_new"));

    let after = call_repo_map_text(&handler);
    assert!(
        after.contains("b.rs"),
        "next call must reflect the external reindex: {after}"
    );
}

#[test]
fn bounded_interval_defers_the_restat_between_reindexes() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("graph.db");
    seed_file_db(&db, "first_sym");

    let handler = McpHandler::open(&db)
        .unwrap()
        .with_recheck_interval(std::time::Duration::from_secs(3600));
    assert!(call_repo_map_text(&handler).contains("a.rs"));

    second_process_index(&dir.path().join("graph.db"), Some("brand_new"));

    // Inside the bound: no reopen — the (still-current) old data answers.
    let text = call_repo_map_text(&handler);
    assert!(text.contains("a.rs"), "{text}");
    assert!(
        !text.contains("b.rs"),
        "must not reopen per-request: {text}"
    );
}

/// The reload path never panics or loses the session — even when the
/// database file vanishes mid-session (unclean state), the handler keeps
/// answering from its last-known-good snapshot.
#[test]
fn vanished_database_never_panics_the_handler() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("graph.db");
    seed_file_db(&db, "first_sym");

    let handler = McpHandler::open(&db)
        .unwrap()
        .with_recheck_interval(std::time::Duration::ZERO);
    assert!(call_repo_map_text(&handler).contains("a.rs"));

    std::fs::remove_file(&db).unwrap();

    let text = call_repo_map_text(&handler);
    assert!(
        text.contains("a.rs"),
        "last-known-good snapshot kept: {text}"
    );
}

/// Regression for a real RBAC bypass: `weave_repo_map`/`weave_file_api`
/// (unlike `weave_trace_calls`/`weave_impact_radius`) never touched the
/// session's `RbacGuard` at all — a masked identity got the full,
/// unredacted source structure through either tool. Both must now agree
/// with `weave_trace_calls` about what a restricted identity can see.
#[cfg(feature = "rbac")]
#[test]
fn repo_map_and_file_api_respect_the_bound_rbac_identity() {
    use weave_graph_core::rbac::{Identity, RbacGuard};

    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_node(&Node {
            id: 1,
            repo_id: "test".to_string(),
            path: "src/payment/core.rs".to_string(),
            symbol: "charge_card".to_string(),
            kind: "function".to_string(),
            line_start: 1,
            line_end: 3,
            signature: "fn charge_card()".to_string(),
        })
        .unwrap();
    storage
        .upsert_node(&Node {
            id: 2,
            repo_id: "test".to_string(),
            path: "src/public/api.rs".to_string(),
            symbol: "list_products".to_string(),
            kind: "function".to_string(),
            line_start: 1,
            line_end: 3,
            signature: "fn list_products()".to_string(),
        })
        .unwrap();

    let identity = Identity {
        subject: "contractor-bot".to_string(),
        roles: vec!["contractor".to_string()],
    };
    let guard = RbacGuard::new(identity, |n: &Node| !n.path.starts_with("src/payment/"));
    let handler = McpHandler::new(storage).unwrap().with_identity(guard);

    let call = |name: &str, arguments: serde_json::Value| -> String {
        let msg = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        })
        .to_string();
        let res = handler.handle_message(&msg).unwrap();
        res.result.unwrap()["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };

    let file_api_text = call(
        "weave_file_api",
        json!({ "paths": ["src/payment/core.rs"] }),
    );
    assert!(
        !file_api_text.contains("charge_card"),
        "weave_file_api leaked a masked symbol: {file_api_text}"
    );
    assert!(file_api_text.contains("<rbac: hidden>"), "{file_api_text}");

    let repo_map_text = call("weave_repo_map", json!({}));
    assert!(
        !repo_map_text.contains("payment") && !repo_map_text.contains("charge_card"),
        "weave_repo_map leaked a masked path/symbol: {repo_map_text}"
    );
    assert!(
        repo_map_text.contains("src/public/api.rs"),
        "{repo_map_text}"
    );
}

/// Doc regression: a failed symbol lookup must be a real MCP tool error
/// (`isError: true`), not a silent success carrying a "not found" string —
/// previously `weave_trace_calls`/`weave_impact_radius` always set
/// `is_error: None` regardless of whether the symbol resolved.
#[test]
fn unresolved_symbol_lookups_set_is_error_true() {
    let storage = setup_storage();
    let handler = McpHandler::new(storage).unwrap();

    for (name, arguments) in [
        ("weave_trace_calls", json!({ "symbol": "does_not_exist" })),
        ("weave_impact_radius", json!({ "symbol": "does_not_exist" })),
    ] {
        let req = json!({
            "jsonrpc": "2.0", "id": 50, "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        })
        .to_string();
        let res = handler.handle_message(&req).unwrap();
        let result = res.result.unwrap();
        assert_eq!(
            result["isError"], true,
            "{name} must set isError:true on an unresolved symbol: {result:?}"
        );
    }
}
