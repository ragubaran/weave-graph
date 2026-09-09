use serde_json::json;

use super::*;

#[test]
fn jsonrpc_request_deserializes_correctly() {
    let raw =
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"test"}}}"#;
    let req: JsonRpcRequest = serde_json::from_str(raw).unwrap();
    assert_eq!(req.jsonrpc, "2.0");
    assert_eq!(req.id, Some(json!(1)));
    assert_eq!(req.method, "initialize");
    assert!(req.params.is_some());
}

#[test]
fn jsonrpc_response_success_serializes_cleanly() {
    let res = JsonRpcResponse::success(Some(json!(42)), json!({"status": "ok"}));
    let val = serde_json::to_value(&res).unwrap();
    assert_eq!(val["jsonrpc"], "2.0");
    assert_eq!(val["id"], 42);
    assert_eq!(val["result"]["status"], "ok");
    assert!(val.get("error").is_none());
}

#[test]
fn jsonrpc_response_error_serializes_cleanly() {
    let res = JsonRpcResponse::error(Some(json!("abc")), -32601, "Method not found");
    let val = serde_json::to_value(&res).unwrap();
    assert_eq!(val["jsonrpc"], "2.0");
    assert_eq!(val["id"], "abc");
    assert_eq!(val["error"]["code"], -32601);
    assert_eq!(val["error"]["message"], "Method not found");
    assert!(val.get("result").is_none());
}

#[test]
fn call_tool_result_constructors_work() {
    let ok = CallToolResult::ok("success content");
    assert_eq!(ok.content.len(), 1);
    assert_eq!(ok.content[0].kind, "text");
    assert_eq!(ok.content[0].text, "success content");
    assert_eq!(ok.is_error, None);

    let err = CallToolResult::err("failure reason");
    assert_eq!(err.content.len(), 1);
    assert_eq!(err.content[0].text, "failure reason");
    assert_eq!(err.is_error, Some(true));
}
