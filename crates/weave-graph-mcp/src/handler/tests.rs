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
    let handler = McpHandler::new(&storage).unwrap();
    let res = handler.handle_message("invalid json").unwrap();
    assert!(res.error.is_some());
    assert_eq!(res.error.unwrap().code, -32700);
}

#[test]
fn handle_initialize() {
    let storage = setup_storage();
    let handler = McpHandler::new(&storage).unwrap();
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
    let handler = McpHandler::new(&storage).unwrap();

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
    let handler = McpHandler::new(&storage).unwrap();
    let msg = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/list"
    })
    .to_string();

    let res = handler.handle_message(&msg).unwrap();
    let tools = res.result.unwrap()["tools"].as_array().unwrap().clone();
    assert_eq!(tools.len(), 4);
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"weave_repo_map"));
    assert!(names.contains(&"weave_file_api"));
    assert!(names.contains(&"weave_trace_calls"));
    assert!(names.contains(&"weave_impact_radius"));
}

#[test]
fn handle_tools_call_all_four_tools() {
    let storage = setup_storage();
    let handler = McpHandler::new(&storage).unwrap();

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

#[test]
fn handle_unknown_tool_and_unknown_method() {
    let storage = setup_storage();
    let handler = McpHandler::new(&storage).unwrap();

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
    let handler = McpHandler::new(&storage).unwrap();

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
