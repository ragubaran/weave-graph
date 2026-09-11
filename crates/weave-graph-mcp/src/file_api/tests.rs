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

    let result = weave_file_api(
        &storage,
        FileApiArgs {
            paths: &["a.rs"],
            max_tokens: None,
        },
    );
    assert_eq!(result.cards.len(), 1);
    assert_eq!(result.cards[0].path, "a.rs");
    assert_eq!(result.cards[0].symbols.len(), 2);
}

#[test]
fn symbols_are_ordered_by_span() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage.upsert_node(&node("a.rs", "fn_b", 20)).unwrap();
    storage.upsert_node(&node("a.rs", "fn_a", 1)).unwrap();

    let result = weave_file_api(
        &storage,
        FileApiArgs {
            paths: &["a.rs"],
            max_tokens: None,
        },
    );
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
            max_tokens: None,
        },
    );
    assert_eq!(result.cards.len(), 1);
    assert!(result.cards[0].symbols.is_empty());
}

// ─── impl.md M2.16: token-budgeted shedding tiers ───────────────────────────

#[test]
fn render_cards_sheds_to_names_then_counts_on_overflow() {
    let card = WiringCard {
        path: "big.rs".into(),
        symbols: (0..30)
            .map(|i| SymbolEntry {
                kind: "function".into(),
                symbol: format!("fn_symbol_{i:02}"),
                span: "L1-2".into(),
                signature: format!("fn fn_symbol_{i}(a: u32, b: u64) -> u64 {{ x }}"),
            })
            .collect(),
    };

    // No budget: byte-identical legacy rendering.
    let full = render_cards(std::slice::from_ref(&card), None);
    assert!(full.contains("  fn_symbol_00 [function] L1-2"));

    // Comfortable budget keeps the full cards.
    let roomy = render_cards(std::slice::from_ref(&card), Some(1000));
    assert_eq!(full, roomy);

    // Tight budget sheds to per-file symbol names.
    let shed = render_cards(std::slice::from_ref(&card), Some(35));
    assert!(shed.contains("big.rs: "), "{shed}");
    assert!(!shed.contains("[function]"), "card detail shed: {shed}");
    assert!(crate::tools::estimate_tokens(&shed) <= 35, "{shed}");

    // Still over → per-file counts.
    let counts = render_cards(std::slice::from_ref(&card), Some(8));
    assert_eq!(counts, "big.rs: 30 symbols\n", "{counts}");
}
