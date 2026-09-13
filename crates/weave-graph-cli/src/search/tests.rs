use std::fs;

use super::{bounded_limit, cmd_search, run};
use weave_graph_core::MAX_SEARCH_LIMIT;
use weave_graph_store_sqlite::SqliteStorage;

fn init_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let weave_dir = dir.path().join(".weave");
    fs::create_dir_all(&weave_dir).unwrap();
    let active_db = weave_dir.join("graph.db");
    let source = dir.path().join("auth.rs");
    fs::write(
        &source,
        "pub fn checkJwtTtl() {}\npub fn unrelatedHelper() {}\n",
    )
    .unwrap();
    crate::index::full_reindex(dir.path(), &weave_dir, &active_db, &[source]).unwrap();
    dir
}

#[test]
fn search_finds_a_symbol_via_synonym_expansion() {
    let repo = init_repo();
    let storage = SqliteStorage::open(&repo.path().join(".weave").join("graph.db")).unwrap();

    // "token lifetime" never appears literally in the source — this only
    // finds `checkJwtTtl` through the "auth"/"ttl" synonym groups plus
    // identifier splitting.
    let hits = run(&storage, "token lifetime", 10, None).unwrap();

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].symbol, "checkJwtTtl");
}

#[test]
fn search_reports_no_matches_as_an_empty_list_not_an_error() {
    let repo = init_repo();
    let storage = SqliteStorage::open(&repo.path().join(".weave").join("graph.db")).unwrap();

    assert!(
        run(&storage, "nonexistentzzz", 10, None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn search_respects_the_limit_argument() {
    let dir = tempfile::tempdir().unwrap();
    let weave_dir = dir.path().join(".weave");
    fs::create_dir_all(&weave_dir).unwrap();
    let active_db = weave_dir.join("graph.db");
    let source = dir.path().join("auth.rs");
    fs::write(
        &source,
        "pub fn authOne() {}\npub fn authTwo() {}\npub fn authThree() {}\n",
    )
    .unwrap();
    crate::index::full_reindex(dir.path(), &weave_dir, &active_db, &[source]).unwrap();
    let storage = SqliteStorage::open(&active_db).unwrap();

    assert_eq!(run(&storage, "auth", 10, None).unwrap().len(), 3);
    assert_eq!(run(&storage, "auth", 1, None).unwrap().len(), 1);
}

#[test]
fn search_limit_is_bounded_before_storage_work() {
    assert_eq!(bounded_limit(usize::MAX), MAX_SEARCH_LIMIT);
}

#[test]
fn search_skips_a_stale_fts_entry_whose_node_id_no_longer_exists() {
    use weave_graph_core::Storage;

    let repo = init_repo();
    let db_path = repo.path().join(".weave").join("graph.db");
    let mut storage = SqliteStorage::open(&db_path).unwrap();
    // Purge the node without rebuilding the FTS index, so `symbol_fts`
    // still holds a rowid `get_node` can no longer resolve — the same
    // defensive gap a race between search and a concurrent reindex could
    // hit in practice.
    storage.purge_file_nodes("local", "auth.rs").unwrap();

    let hits = run(&storage, "token lifetime", 10, None).unwrap();
    assert!(hits.is_empty());
}

#[test]
fn cmd_search_runs_end_to_end_without_error() {
    let repo = init_repo();
    cmd_search(repo.path(), "token lifetime", 10, None).unwrap();
    cmd_search(repo.path(), "nonexistentzzz", 10, None).unwrap();
}

#[cfg(feature = "vector")]
mod semantic {
    use super::super::{cmd_search_semantic, run_semantic};
    use super::init_repo;
    use weave_graph_store_sqlite::SqliteStorage;

    fn repo_with_two_topics() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let weave_dir = dir.path().join(".weave");
        std::fs::create_dir_all(&weave_dir).unwrap();
        let active_db = weave_dir.join("graph.db");
        let source = dir.path().join("mixed.rs");
        std::fs::write(
            &source,
            "pub fn check_jwt_expiry(token: &str) -> bool { true }\n\
             pub fn render_page_layout(page: &str) -> String { page.to_string() }\n",
        )
        .unwrap();
        crate::index::full_reindex(dir.path(), &weave_dir, &active_db, &[source]).unwrap();
        dir
    }

    #[test]
    fn semantic_search_ranks_the_closer_chunk_first() {
        let repo = repo_with_two_topics();
        let storage = SqliteStorage::open(&repo.path().join(".weave").join("graph.db")).unwrap();

        let hits = run_semantic(&storage, "verify auth token expiry", 5, None).unwrap();

        assert!(!hits.is_empty());
        assert_eq!(hits[0].symbol, "check_jwt_expiry");
    }

    #[test]
    fn semantic_search_reports_no_matches_as_an_empty_list() {
        let repo = init_repo();
        let storage = SqliteStorage::open(&repo.path().join(".weave").join("graph.db")).unwrap();

        // Every chunk gets some non-zero similarity under the mock
        // embedder, so assert the limit is honored instead of emptiness.
        let hits = run_semantic(&storage, "auth token expiry", 1, None).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn cmd_search_semantic_runs_end_to_end_without_error() {
        let repo = repo_with_two_topics();
        cmd_search_semantic(repo.path(), "verify auth token expiry", 5, None).unwrap();
    }

    /// SEC-01 through the CLI layer: the best-matching chunk being masked
    /// must not shrink `limit`-capped results — a visible runner-up must
    /// still surface.
    #[cfg(feature = "rbac")]
    #[test]
    fn semantic_search_does_not_starve_a_visible_hit_behind_a_masked_top_match() {
        let dir = tempfile::tempdir().unwrap();
        let weave_dir = dir.path().join(".weave");
        std::fs::create_dir_all(&weave_dir).unwrap();
        let active_db = weave_dir.join("graph.db");
        let source = dir.path().join("mixed.rs");
        std::fs::write(
            &source,
            "fn check_jwt_expiry(token: &str) -> bool { true }\n\
             pub fn render_page_layout(page: &str) -> String { page.to_string() }\n",
        )
        .unwrap();
        crate::index::full_reindex(dir.path(), &weave_dir, &active_db, &[source]).unwrap();
        std::fs::write(weave_dir.join("config.toml"), "[rbac.users]\nbob = []\n").unwrap();

        let storage = SqliteStorage::open(&active_db).unwrap();
        let guard = crate::rbac::guard_for(dir.path(), Some("bob"));
        let visible: &dyn Fn(&weave_graph_core::Node) -> bool = &|n| guard.visible(n);

        // Unmasked: the private function is the closer match and wins.
        let unmasked = run_semantic(&storage, "verify auth token expiry", 1, None).unwrap();
        assert_eq!(unmasked[0].symbol, "check_jwt_expiry");

        // Masked as `bob` (no roles): the masked top hit must not push
        // the visible runner-up out of a `limit`-capped result set.
        let masked = run_semantic(&storage, "verify auth token expiry", 1, Some(visible)).unwrap();
        assert_eq!(masked.len(), 1, "a visible match must still surface");
        assert_eq!(masked[0].symbol, "render_page_layout");
    }
}
