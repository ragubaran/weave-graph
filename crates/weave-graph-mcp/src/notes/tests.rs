use super::*;
use weave_graph_core::Storage;
use weave_graph_store_sqlite::SqliteStorage;

fn seed_storage() -> SqliteStorage {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_node(&weave_graph_core::Node {
            id: 0,
            repo_id: "r".into(),
            path: "a.rs".into(),
            symbol: "alpha".into(),
            kind: "function".into(),
            line_start: 1,
            line_end: 3,
            signature: "fn alpha()".into(),
        })
        .unwrap();
    storage
}

/// weave_pin_note persists with the crystallized tier from its argument;
/// weave_recall_notes reports live notes with tier/stale/orphan markers.
#[test]
#[cfg(feature = "notes")]
fn pin_then_recall_round_trips_through_the_tool_pair() {
    let storage = seed_storage();
    let out = weave_pin_note(
        &storage,
        Path::new("."),
        PinNoteArgs {
            symbol: "alpha",
            text: "why alpha exists",
            tier: Some("crystallized"),
            kind: Some("arch_decision"),
        },
    );
    assert!(out.starts_with("Pinned note #1"), "{out}");

    let recalled = weave_recall_notes(&storage);
    assert!(recalled.contains("#1 [crystallized]"), "{recalled}");
    assert!(recalled.contains("a.rs#alpha"), "{recalled}");
    assert!(recalled.contains("why alpha exists"));
}

#[test]
#[cfg(feature = "notes")]
fn pinning_an_unknown_symbol_is_a_clear_error() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let out = weave_pin_note(
        &storage,
        Path::new("."),
        PinNoteArgs {
            symbol: "nope",
            text: "x",
            tier: None,
            kind: None,
        },
    );
    assert_eq!(out, "symbol not found: nope");
}

/// Full handler round trip: tools/call weave_pin_note then
/// weave_recall_notes over the MCP protocol surface.
#[test]
#[cfg(feature = "notes")]
fn handler_routes_the_notes_tool_pair_end_to_end() {
    #[cfg(feature = "notes")]
    use crate::handler::McpHandler;
    #[cfg(feature = "notes")]
    use serde_json::json;
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_node(&weave_graph_core::Node {
            id: 0,
            repo_id: "r".into(),
            path: "a.rs".into(),
            symbol: "alpha".into(),
            kind: "function".into(),
            line_start: 1,
            line_end: 3,
            signature: String::new(),
        })
        .unwrap();
    let handler = McpHandler::new(storage).unwrap();

    let pin = handler.handle_message(
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
            "name":"weave_pin_note",
            "arguments":{"symbol":"alpha","text":"rationale","tier":"crystallized"}
        }})
        .to_string(),
    );
    assert!(pin.is_some());

    let recall = handler.handle_message(
        &json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
            "name":"weave_recall_notes","arguments":{}
        }})
        .to_string(),
    );
    let recall = recall.unwrap();
    assert!(
        serde_json::to_string(&recall)
            .unwrap()
            .contains("a.rs#alpha")
    );
}
