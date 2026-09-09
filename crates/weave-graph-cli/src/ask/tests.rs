use weave_graph_core::Node;

use super::*;

fn node(symbol: &str, id: u32) -> Node {
    Node {
        id,
        repo_id: "local".to_string(),
        path: "src/a.rs".to_string(),
        symbol: symbol.to_string(),
        kind: "function".to_string(),
        line_start: 1,
        line_end: 3,
        signature: String::new(),
    }
}

fn nodes() -> Vec<Node> {
    vec![
        node("helper", 1),
        node("verifyJWTSession", 2),
        node("event_bus", 3),
        node("event_sink", 4),
    ]
}

#[test]
fn exact_symbol_grouns_to_itself() {
    let nodes = nodes();
    assert!(ground_symbol(&nodes, "helper").is_ok());
}

#[test]
fn case_insensitive_near_miss_corrects() {
    let nodes = nodes();
    let id = ground_symbol(&nodes, "jwt")
        .map_err(|(name, _)| name)
        .unwrap();
    assert_eq!(
        nodes.iter().find(|n| n.id == id).map(|n| n.symbol.as_str()),
        Some("verifyJWTSession")
    );
}

#[test]
fn ambiguous_or_missing_symbol_is_reported_not_found_never_dispatched() {
    let nodes = nodes();
    // event_* are two substring hits — ambiguous stays unresolved.
    assert!(ground_symbol(&nodes, "event").is_err());
    let (name, closest) = ground_symbol(&nodes, "nonexistent").unwrap_err();
    assert_eq!(name, "nonexistent");
    assert!(closest.is_empty());
}

#[test]
fn grounded_call_corrects_symbols_and_preserves_the_tool() {
    let nodes = nodes();
    let routed = RoutedCall {
        tool: "impact".to_string(),
        symbol: "jwt".to_string(),
        second: None,
    };
    let grounded = ground_call(&nodes, &routed).unwrap();
    assert_eq!(grounded.tool, "impact");
    assert_eq!(grounded.symbol, "verifyJWTSession");
    assert_ne!(grounded, routed, "near-miss correction must be visible");
}

#[test]
fn invented_symbol_is_refused_with_candidates_named() {
    let nodes = nodes();
    let routed = RoutedCall {
        tool: "callers".to_string(),
        symbol: "madeUpThing".to_string(),
        second: None,
    };
    let err = ground_call(&nodes, &routed).unwrap_err();
    assert!(
        err.contains("symbol not found in index: madeUpThing"),
        "{err}"
    );
}
