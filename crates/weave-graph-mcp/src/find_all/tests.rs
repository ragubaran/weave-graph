use super::*;
use crate::tools::FindAllArgs;
use weave_graph_core::Storage;
use weave_graph_store_sqlite::SqliteStorage;

fn node(symbol: &str, path: &str, kind: &str, signature: &str) -> weave_graph_core::Node {
    weave_graph_core::Node {
        id: 0,
        repo_id: "local".into(),
        path: path.into(),
        symbol: symbol.into(),
        kind: kind.into(),
        line_start: 1,
        line_end: 2,
        signature: signature.into(),
    }
}

fn seeded_storage() -> SqliteStorage {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_node(&node(
            "checkAuth",
            "src/auth.rs",
            "function",
            "fn checkAuth() { validate() }",
        ))
        .unwrap();
    storage
        .upsert_node(&node(
            "AuthStruct",
            "src/auth.rs",
            "struct",
            "struct AuthStruct { valid: bool }",
        ))
        .unwrap();
    storage
        .upsert_node(&node(
            "renderPage",
            "src/ui.py",
            "function",
            "def renderPage(): return auth_check()",
        ))
        .unwrap();
    storage.rebuild_fts_index().unwrap();
    storage
}

#[test]
fn finds_every_match_across_kinds_and_files() {
    let storage = seeded_storage();
    let result = weave_find_all(
        &storage,
        FindAllArgs {
            pattern: "auth",
            path: None,
            language: None,
            kind: None,
            limit: 50,
            max_tokens: None,
        },
        None,
    );
    assert_eq!(result.total_matches, 3, "{}", result.text);
    assert!(result.text.contains("checkAuth"), "{}", result.text);
    assert!(result.text.contains("AuthStruct"), "{}", result.text);
}

#[test]
fn path_filter_narrows_to_one_file() {
    let storage = seeded_storage();
    let result = weave_find_all(
        &storage,
        FindAllArgs {
            pattern: "auth",
            path: Some("src/ui"),
            language: None,
            kind: None,
            limit: 50,
            max_tokens: None,
        },
        None,
    );
    assert_eq!(result.total_matches, 1, "{}", result.text);
    assert!(result.text.contains("renderPage"), "{}", result.text);
}

#[test]
fn language_filter_matches_by_extension() {
    let storage = seeded_storage();
    let result = weave_find_all(
        &storage,
        FindAllArgs {
            pattern: "auth",
            path: None,
            language: Some("python"),
            kind: None,
            limit: 50,
            max_tokens: None,
        },
        None,
    );
    assert_eq!(result.total_matches, 1, "{}", result.text);
    assert!(result.text.contains("renderPage"), "{}", result.text);
}

#[test]
fn kind_filter_narrows_to_structs_only() {
    let storage = seeded_storage();
    let result = weave_find_all(
        &storage,
        FindAllArgs {
            pattern: "auth",
            path: None,
            language: None,
            kind: Some("struct"),
            limit: 50,
            max_tokens: None,
        },
        None,
    );
    assert_eq!(result.total_matches, 1, "{}", result.text);
    assert!(result.text.contains("AuthStruct"), "{}", result.text);
}

#[test]
fn total_matches_stays_exhaustive_even_when_limit_truncates_the_rendered_list() {
    let storage = seeded_storage();
    let result = weave_find_all(
        &storage,
        FindAllArgs {
            pattern: "auth",
            path: None,
            language: None,
            kind: None,
            limit: 1,
            max_tokens: None,
        },
        None,
    );
    assert_eq!(result.total_matches, 3);
    assert!(result.text.contains("and 2 more"), "{}", result.text);
}

#[test]
fn no_match_reports_zero_not_an_error() {
    let storage = seeded_storage();
    let result = weave_find_all(
        &storage,
        FindAllArgs {
            pattern: "nonexistentzzz",
            path: None,
            language: None,
            kind: None,
            limit: 50,
            max_tokens: None,
        },
        None,
    );
    assert_eq!(result.total_matches, 0);
    assert!(result.text.contains("no matches"), "{}", result.text);
}

#[test]
fn mask_is_applied_to_every_result() {
    let storage = seeded_storage();
    let mask: &dyn Fn(&weave_graph_core::Node) -> weave_graph_core::Node =
        &|n| weave_graph_core::Node {
            symbol: format!("masked-{}", n.symbol),
            ..n.clone()
        };
    let result = weave_find_all(
        &storage,
        FindAllArgs {
            pattern: "auth",
            path: None,
            language: None,
            kind: None,
            limit: 50,
            max_tokens: None,
        },
        Some(mask),
    );
    assert!(result.text.contains("masked-checkAuth"), "{}", result.text);
}

#[test]
fn a_tiny_max_tokens_still_reports_the_true_exhaustive_count() {
    let storage = seeded_storage();
    let result = weave_find_all(
        &storage,
        FindAllArgs {
            pattern: "auth",
            path: None,
            language: None,
            kind: None,
            limit: 50,
            max_tokens: Some(1),
        },
        None,
    );
    assert_eq!(result.total_matches, 3, "{}", result.text);
}
