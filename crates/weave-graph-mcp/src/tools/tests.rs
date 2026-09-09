use super::*;
use weave_graph_core::Node;

fn fake_node(id: u32, symbol: &str) -> Node {
    Node {
        id,
        repo_id: "r".into(),
        path: "a.rs".into(),
        symbol: symbol.into(),
        kind: "function".into(),
        line_start: 1,
        line_end: 5,
        signature: format!("fn {symbol}()"),
    }
}

#[test]
fn resolve_symbol_finds_first_match() {
    let nodes = vec![
        fake_node(1, "foo"),
        fake_node(2, "bar"),
        fake_node(3, "foo"),
    ];
    assert_eq!(resolve_symbol(&nodes, "foo"), Some(1));
    assert_eq!(resolve_symbol(&nodes, "bar"), Some(2));
    assert_eq!(resolve_symbol(&nodes, "baz"), None);
}

#[test]
fn symbol_entry_formats_span_correctly() {
    let node = fake_node(1, "fn_a");
    let entry = SymbolEntry::from_node(&node);
    assert_eq!(entry.span, "L1-5");
    assert_eq!(entry.kind, "function");
}

#[test]
fn repo_map_args_default_sets_50_files() {
    let args = RepoMapArgs::default();
    assert_eq!(args.max_files, 50);
}
