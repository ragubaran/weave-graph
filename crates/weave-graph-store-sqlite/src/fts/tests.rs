use weave_graph_core::{Node, Storage};

use crate::SqliteStorage;

fn node(symbol: &str, signature: &str) -> Node {
    Node {
        id: 0,
        repo_id: "local".to_string(),
        path: "src/lib.rs".to_string(),
        symbol: symbol.to_string(),
        kind: "function".to_string(),
        line_start: 1,
        line_end: 2,
        signature: signature.to_string(),
    }
}

fn node_at(symbol: &str, path: &str, signature: &str) -> Node {
    Node {
        path: path.to_string(),
        ..node(symbol, signature)
    }
}

#[test]
fn rebuild_then_search_finds_a_split_camel_case_identifier() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_node(&node("checkJwtTtl", "fn checkJwtTtl()"))
        .unwrap();
    storage
        .upsert_node(&node("unrelatedHelper", "fn unrelatedHelper()"))
        .unwrap();
    storage.rebuild_fts_index().unwrap();

    let expanded = weave_graph_core::synonym::expand_query("token lifetime");
    let hits = storage.search_symbols(&expanded, 10, None).unwrap();

    assert_eq!(hits.len(), 1);
    let hit = storage.get_node(hits[0]).unwrap().unwrap();
    assert_eq!(hit.symbol, "checkJwtTtl");
}

#[test]
fn rebuild_clears_stale_entries_after_the_node_is_purged() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_node(&node("authLogin", "fn authLogin()"))
        .unwrap();
    storage.rebuild_fts_index().unwrap();
    assert_eq!(
        storage.search_symbols("\"login\"", 10, None).unwrap().len(),
        1
    );

    // A rebuild after the underlying node is gone must not still surface
    // it — `symbol_fts` is a derived index, never its own source of truth.
    storage.purge_file_nodes("local", "src/lib.rs").unwrap();
    storage.rebuild_fts_index().unwrap();

    assert!(
        storage
            .search_symbols("\"login\"", 10, None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn search_respects_the_limit() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    for i in 0..5 {
        storage
            .upsert_node(&node(&format!("authHandler{i}"), "fn f()"))
            .unwrap();
    }
    storage.rebuild_fts_index().unwrap();

    let hits = storage.search_symbols("\"auth\"", 2, None).unwrap();
    assert_eq!(hits.len(), 2);
}

#[test]
fn search_with_no_match_returns_empty() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_node(&node("authLogin", "fn authLogin()"))
        .unwrap();
    storage.rebuild_fts_index().unwrap();

    assert!(
        storage
            .search_symbols("\"nonexistentterm\"", 10, None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn node_writes_and_purges_maintain_fts_without_a_full_rebuild() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_node(&node("authLogin", "fn authLogin()"))
        .unwrap();

    assert_eq!(
        storage.search_symbols("\"login\"", 10, None).unwrap().len(),
        1
    );

    storage.purge_file_nodes("local", "src/lib.rs").unwrap();

    assert!(
        storage
            .search_symbols("\"login\"", 10, None)
            .unwrap()
            .is_empty()
    );
}

/// A masked-heavy ranking must not underfill `limit`: the adaptive
/// over-fetch keeps widening the candidate window until `limit` visible
/// hits surface or the index is exhausted (query-layer RBAC retained).
#[test]
fn masked_heavy_ranking_still_fills_the_limit_with_visible_hits() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    // 30 masked hits rank ahead of 3 visible ones.
    for i in 0..30 {
        storage
            .upsert_node(&node_at(
                &format!("privateAuth{i}"),
                "private.rs",
                &format!("fn privateAuth{i}() {{ auth auth auth {i} }}"),
            ))
            .unwrap();
    }
    for i in 0..3 {
        storage
            .upsert_node(&node_at(
                &format!("publicAuth{i}"),
                "public.rs",
                &format!("fn publicAuth{i}() {{ auth }}"),
            ))
            .unwrap();
    }

    let hits = storage
        .search_symbol_nodes("\"auth\"", 3, Some(&|node| node.path == "public.rs"))
        .unwrap();

    assert_eq!(hits.len(), 3, "masked-heavy ranking must not underfill");
    assert!(
        hits.iter().all(|n| n.path == "public.rs"),
        "no masked hit may leak"
    );
}

/// An on-disk DB built by an older binary has `symbol_fts`'s old 2-column
/// shape. Opening it with the current binary must detect the shape
/// mismatch, drop-and-rebuild rather than silently keep stale columns,
/// and re-populate `symbol_name`/`signature` from `nodes` without losing
/// searchability — the exact migration path M4B.1/M4B.5 rely on.
#[test]
fn ensure_fts_table_migrates_an_old_shape_table_and_repopulates_from_nodes() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    crate::schema::migrate(&conn).unwrap();
    conn.execute_batch(
        "CREATE VIRTUAL TABLE symbol_fts USING fts5(symbol_name, signature, tokenize = 'porter unicode61');
         INSERT INTO nodes(id, repo_id, path, symbol, kind, line_start, line_end, signature)
             VALUES (1, 'local', 'src/lib.rs', 'authLogin', 'function', 1, 2, 'fn authLogin()');
         INSERT INTO symbol_fts(rowid, symbol_name, signature) VALUES (1, 'auth Login', 'fn authLogin()');",
    )
    .unwrap();

    crate::fts::ensure_fts_table(&conn).unwrap();

    let hits = crate::fts::search_nodes(&conn, "\"login\"", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].symbol, "authLogin");
}

#[test]
fn ensure_fts_table_is_a_no_op_when_the_shape_already_matches() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    crate::schema::migrate(&conn).unwrap();
    crate::fts::ensure_fts_table(&conn).unwrap();
    conn.execute(
        "INSERT INTO nodes(id, repo_id, path, symbol, kind, line_start, line_end, signature) \
         VALUES (1, 'local', 'src/lib.rs', 'authLogin', 'function', 1, 2, 'fn authLogin()')",
        [],
    )
    .unwrap();
    // `upsert_text` is now a partial-column UPDATE — it needs the row's
    // symbol_name/signature already written (as production always does
    // via `replace_row`, never `upsert_text` alone), or there's nothing
    // for it to update.
    crate::fts::replace_row(&conn, 1, "authLogin", "fn authLogin()").unwrap();
    crate::fts::upsert_text(&conn, 1, "", "").unwrap();

    // Re-running ensure_fts_table on an already-current shape must not
    // drop the table and lose the row just written.
    crate::fts::ensure_fts_table(&conn).unwrap();

    let hits = crate::fts::search_nodes(&conn, "\"login\"", 10).unwrap();
    assert_eq!(hits.len(), 1);
}

#[test]
fn upsert_text_makes_body_and_doc_comment_findable() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let id = storage
        .upsert_node(&node("loadConfig", "fn loadConfig()"))
        .unwrap();
    storage.rebuild_fts_index().unwrap();
    storage
        .upsert_fts_text(
            id,
            "fn loadConfig() { read_file_from_disk() }",
            "Reads application settings from disk on startup.",
        )
        .unwrap();

    let body_hits = storage
        .search_symbols("\"read_file_from_disk\"", 10, None)
        .unwrap();
    assert_eq!(body_hits, vec![id]);

    let doc_hits = storage.search_symbols("\"startup\"", 10, None).unwrap();
    assert_eq!(doc_hits, vec![id]);
}

#[test]
fn upsert_text_does_not_spuriously_match_an_unrelated_symbol() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let documented_id = storage
        .upsert_node(&node("loadConfig", "fn loadConfig()"))
        .unwrap();
    let plain_id = storage
        .upsert_node(&node_at("renderPage", "view.rs", "fn renderPage()"))
        .unwrap();
    storage.rebuild_fts_index().unwrap();
    storage
        .upsert_fts_text(
            documented_id,
            "fn loadConfig() { read_file_from_disk() }",
            "Reads application settings from disk on startup.",
        )
        .unwrap();
    // `renderPage` never gets an `upsert_fts_text` call — its body/
    // doc_comment columns stay empty, so it must never surface for a
    // query that only matches the *other* node's backfilled text.
    let _ = plain_id;

    let hits = storage.search_symbols("\"startup\"", 10, None).unwrap();
    assert_eq!(hits, vec![documented_id]);
}

#[test]
fn search_filters_masked_hits_before_applying_the_limit() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    storage
        .upsert_node(&node_at(
            "privateAuth",
            "private.rs",
            "fn privateAuth() { auth auth auth }",
        ))
        .unwrap();
    storage
        .upsert_node(&node_at(
            "publicAuth",
            "public.rs",
            "fn publicAuth() { auth }",
        ))
        .unwrap();

    let hits = storage
        .search_symbol_nodes("\"auth\"", 1, Some(&|node| node.path == "public.rs"))
        .unwrap();

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].symbol, "publicAuth");
}
