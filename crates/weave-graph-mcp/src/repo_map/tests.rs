use super::*;
use crate::tools::{FileApiResult, RepoMapArgs};
use weave_graph_core::{Edge, Node, Storage};
use weave_graph_store_sqlite::SqliteStorage;

fn node(path: &str, symbol: &str) -> Node {
    Node {
        id: 0,
        repo_id: "r".into(),
        path: path.into(),
        symbol: symbol.into(),
        kind: "function".into(),
        line_start: 1,
        line_end: 5,
        signature: format!("fn {symbol}()"),
    }
}
fn edge(src: u32, tgt: u32) -> Edge {
    Edge {
        id: 0,
        source_id: src,
        target_id: tgt,
        kind: "CALLS_EXACT".into(),
        weight: 1.0,
        extractor: None,
        resolution_kind: None,
    }
}

#[test]
fn repo_map_ranks_by_degree_and_respects_max_files() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let id1 = storage.upsert_node(&node("hub.rs", "hub")).unwrap();
    let id2 = storage.upsert_node(&node("leaf.rs", "leaf")).unwrap();
    let id3 = storage.upsert_node(&node("hub.rs", "hub2")).unwrap();
    storage.upsert_edge(&edge(id1, id2)).unwrap();
    storage.upsert_edge(&edge(id3, id2)).unwrap();

    let csr = CsrGraph::load(&storage).unwrap();
    let result = weave_repo_map(
        &storage,
        &csr,
        RepoMapArgs {
            max_files: 10,
            module: None,
            max_tokens: None,
        },
        None,
    );
    assert!(result.text.contains("hub.rs"), "hub.rs must appear");
    assert!(
        result.text.find("hub.rs") < result.text.find("leaf.rs"),
        "hub before leaf"
    );
}

#[test]
fn repo_map_truncates_to_max_files() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    for i in 1..=5 {
        storage
            .upsert_node(&node(&format!("f{i}.rs"), &format!("s{i}")))
            .unwrap();
    }
    let csr = CsrGraph::load(&storage).unwrap();
    let result = weave_repo_map(
        &storage,
        &csr,
        RepoMapArgs {
            max_files: 2,
            module: None,
            max_tokens: None,
        },
        None,
    );
    let file_lines = result
        .text
        .lines()
        .filter(|l| l.trim_start().starts_with('f'))
        .count();
    assert_eq!(file_lines, 2);
}

/// Fixture: two dense modules (src/parser: a/b/c.rs wired together;
/// src/render: x/y.rs wired together) plus one isolated file. Used by the
/// budget/coverage, byte-identical-default, and drill-down tests below.
struct ModuleFixture {
    storage: SqliteStorage,
    csr: CsrGraph,
}

fn module_fixture() -> ModuleFixture {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    // src/parser module: a -> b -> c, plus a -> c (dense).
    let a = storage
        .upsert_node(&node("src/parser/a.rs", "parse_a"))
        .unwrap();
    let b = storage
        .upsert_node(&node("src/parser/b.rs", "parse_b"))
        .unwrap();
    let c = storage
        .upsert_node(&node("src/parser/c.rs", "parse_c"))
        .unwrap();
    storage.upsert_edge(&edge(a, b)).unwrap();
    storage.upsert_edge(&edge(b, c)).unwrap();
    storage.upsert_edge(&edge(a, c)).unwrap();
    // src/render module: x -> y.
    let x = storage
        .upsert_node(&node("src/render/x.rs", "render_x"))
        .unwrap();
    let y = storage
        .upsert_node(&node("src/render/y.rs", "render_y"))
        .unwrap();
    storage.upsert_edge(&edge(x, y)).unwrap();
    // One cross-module edge so modules aren't fully isolated.
    storage.upsert_edge(&edge(c, x)).unwrap();
    // One isolated file (own module).
    let _lone = storage.upsert_node(&node("main.rs", "main")).unwrap();

    let csr = CsrGraph::load(&storage).unwrap();
    ModuleFixture { storage, csr }
}

fn module_args() -> RepoMapArgs {
    RepoMapArgs {
        max_files: 50,
        module: Some(true),
        max_tokens: None,
    }
}

#[test]
fn module_map_covers_all_files_within_orientation_budget() {
    let fx = module_fixture();
    let result = weave_repo_map(&fx.storage, &fx.csr, module_args(), None);

    // 100% file coverage: every indexed file appears in some module line.
    for path in [
        "src/parser/a.rs",
        "src/parser/b.rs",
        "src/parser/c.rs",
        "src/render/x.rs",
        "src/render/y.rs",
        "main.rs",
    ] {
        assert!(
            result.text.contains(path),
            "{path} must appear in module membership"
        );
    }

    // ~200-token orientation budget (chars/4 heuristic): 7 short lines.
    let approx_tokens = result.text.len() / 4;
    assert!(
        approx_tokens <= 200,
        "module map must stay within ~200 tokens, got ~{approx_tokens}:\n{}",
        result.text
    );

    // Module lines carry label, file count, symbol count, cross-edges.
    assert!(result.text.contains("repo map ("), "header expected");
    assert!(
        result.text.contains("cross-edges"),
        "aggregate weight expected"
    );
}

#[test]
fn module_map_labels_modules_by_shared_directory() {
    let fx = module_fixture();
    let result = weave_repo_map(&fx.storage, &fx.csr, module_args(), None);
    assert!(result.text.contains("src/parser"), "parser module label");
    assert!(result.text.contains("src/render"), "render module label");
}

#[test]
fn file_level_default_is_byte_identical_without_module_flag() {
    let fx = module_fixture();
    // None and Some(false) must produce the same output…
    let none = weave_repo_map(
        &fx.storage,
        &fx.csr,
        RepoMapArgs {
            max_files: 50,
            module: None,
            max_tokens: None,
        },
        None,
    );
    let some_false = weave_repo_map(
        &fx.storage,
        &fx.csr,
        RepoMapArgs {
            max_files: 50,
            module: Some(false),
            max_tokens: None,
        },
        None,
    );
    assert_eq!(none.text, some_false.text);
    // …and that output is the file-level shape, not module lines.
    assert!(none.text.contains("repo map ("));
    assert!(
        !none.text.contains("cross-edges"),
        "default must stay file-level"
    );
}

/// Drill-down regression: module → file → symbol reaches
/// the exact wiring cards the file-level path returns for the same files.
#[test]
fn module_drill_down_reaches_the_same_wiring_cards_as_the_file_level_path() {
    use crate::file_api::weave_file_api;
    use crate::tools::FileApiArgs;

    let fx = module_fixture();
    let map = weave_repo_map(&fx.storage, &fx.csr, module_args(), None);

    // Parse every file named in the module map's membership lists.
    let all_paths = [
        "src/parser/a.rs",
        "src/parser/b.rs",
        "src/parser/c.rs",
        "src/render/x.rs",
        "src/render/y.rs",
        "main.rs",
    ];
    let module_files: Vec<&str> = all_paths
        .iter()
        .copied()
        .filter(|p| map.text.contains(p))
        .collect();
    assert_eq!(
        module_files.len(),
        all_paths.len(),
        "every file must be reachable from the module map"
    );

    // Drill-down cards (module → files) vs. the file-level path's cards
    // for the same paths: identical wiring cards, byte-for-byte.
    let drill = weave_file_api(
        &fx.storage,
        FileApiArgs {
            paths: &module_files,
            max_tokens: None,
        },
        None,
    );
    let direct = weave_file_api(
        &fx.storage,
        FileApiArgs {
            paths: &all_paths,
            max_tokens: None,
        },
        None,
    );
    assert_wiring_cards_equal(&drill, &direct);
}

fn assert_wiring_cards_equal(a: &FileApiResult, b: &FileApiResult) {
    assert_eq!(a.cards.len(), b.cards.len());
    for (ca, cb) in a.cards.iter().zip(&b.cards) {
        assert_eq!(ca.path, cb.path);
        assert_eq!(ca.symbols, cb.symbols);
    }
}

// ─── token-budgeted truncation ───────────────────────────────

#[test]
fn max_tokens_replaces_max_files_truncation_when_set() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    for i in 1..=10 {
        storage
            .upsert_node(&node(&format!("f{i}.rs"), &format!("s{i}")))
            .unwrap();
    }
    let csr = CsrGraph::load(&storage).unwrap();

    // max_files=10 alone would list all 10 files; a tight token budget sheds.
    let result = weave_repo_map(
        &storage,
        &csr,
        RepoMapArgs {
            max_files: 10,
            module: None,
            max_tokens: Some(20),
        },
        None,
    );
    assert!(
        crate::tools::estimate_tokens(&result.text) <= 20,
        "{}",
        result.text
    );
    assert!(
        result.text.lines().count() > 1,
        "at least one file line: {}",
        result.text
    );

    // Omitting max_tokens: max_files behavior unchanged (byte-identical
    // to the un-truncated format).
    let default_result = weave_repo_map(
        &storage,
        &csr,
        RepoMapArgs {
            max_files: 3,
            module: None,
            max_tokens: None,
        },
        None,
    );
    let file_lines = default_result
        .text
        .lines()
        .filter(|l| l.trim_start().starts_with('f'))
        .count();
    assert_eq!(file_lines, 3);
}

// ─── RBAC masking (was previously entirely unenforced at the query layer) ───

fn hide_payment_files(n: &Node) -> Node {
    if n.path.starts_with("src/payment/") {
        Node {
            id: n.id,
            repo_id: n.repo_id.clone(),
            path: "<rbac: hidden>".to_string(),
            symbol: "<rbac: hidden>".to_string(),
            kind: "<rbac: hidden>".to_string(),
            line_start: 0,
            line_end: 0,
            signature: String::new(),
        }
    } else {
        n.clone()
    }
}

#[test]
fn file_level_repo_map_never_names_a_masked_path() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_node(&node("src/payment/core.rs", "charge_card"))
        .unwrap();
    storage
        .upsert_node(&node("src/public/api.rs", "list_products"))
        .unwrap();
    let csr = CsrGraph::load(&storage).unwrap();

    let result = weave_repo_map(
        &storage,
        &csr,
        RepoMapArgs {
            max_files: 10,
            module: None,
            max_tokens: None,
        },
        Some(&hide_payment_files),
    );
    assert!(
        !result.text.contains("payment") && !result.text.contains("charge_card"),
        "masked path/symbol must never appear: {}",
        result.text
    );
    assert!(result.text.contains("src/public/api.rs"), "{}", result.text);
    assert!(result.text.contains("<rbac: hidden>"), "{}", result.text);
}

#[test]
fn module_level_repo_map_folds_masked_files_into_one_hidden_bucket() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let a = storage
        .upsert_node(&node("src/payment/core.rs", "charge_card"))
        .unwrap();
    let b = storage
        .upsert_node(&node("src/payment/refund.rs", "refund_card"))
        .unwrap();
    storage.upsert_edge(&edge(a, b)).unwrap();
    storage
        .upsert_node(&node("src/public/api.rs", "list_products"))
        .unwrap();
    let csr = CsrGraph::load(&storage).unwrap();

    let result = weave_repo_map(
        &storage,
        &csr,
        RepoMapArgs {
            max_files: 50,
            module: Some(true),
            max_tokens: None,
        },
        Some(&hide_payment_files),
    );
    assert!(
        !result.text.contains("payment") && !result.text.contains("charge_card"),
        "masked path/symbol must never appear in module membership: {}",
        result.text
    );
    assert!(result.text.contains("src/public/api.rs"), "{}", result.text);
    assert!(result.text.contains("<rbac: hidden>"), "{}", result.text);
}
