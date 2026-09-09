use super::*;
use crate::tools::FileApiArgs;
use weave_graph_core::{Node, Storage};
use weave_graph_store_sqlite::SqliteStorage;

fn node(path: &str, symbol: &str, line: u32) -> Node {
    Node {
        id: 0,
        repo_id: "r".into(),
        path: path.into(),
        symbol: symbol.into(),
        kind: "function".into(),
        line_start: line,
        line_end: line + 4,
        signature: format!("fn {symbol}()"),
    }
}

#[test]
fn returns_only_requested_file_symbols() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage.upsert_node(&node("a.rs", "fn_a", 1)).unwrap();
    storage.upsert_node(&node("b.rs", "fn_b", 1)).unwrap();
    storage.upsert_node(&node("a.rs", "fn_c", 10)).unwrap();

    let result = weave_file_api(&storage, FileApiArgs { paths: &["a.rs"] });
    assert_eq!(result.cards.len(), 1);
    assert_eq!(result.cards[0].path, "a.rs");
    assert_eq!(result.cards[0].symbols.len(), 2);
}

#[test]
fn symbols_are_ordered_by_span() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage.upsert_node(&node("a.rs", "fn_b", 20)).unwrap();
    storage.upsert_node(&node("a.rs", "fn_a", 1)).unwrap();

    let result = weave_file_api(&storage, FileApiArgs { paths: &["a.rs"] });
    assert_eq!(result.cards[0].symbols[0].symbol, "fn_a");
    assert_eq!(result.cards[0].symbols[1].symbol, "fn_b");
}

#[test]
fn missing_path_returns_empty_card() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage.upsert_node(&node("a.rs", "fn_a", 1)).unwrap();

    let result = weave_file_api(
        &storage,
        FileApiArgs {
            paths: &["missing.rs"],
        },
    );
    assert_eq!(result.cards.len(), 1);
    assert!(result.cards[0].symbols.is_empty());
}
