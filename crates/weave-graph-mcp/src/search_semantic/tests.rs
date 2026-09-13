use super::*;
use weave_graph_core::embedding::MockEmbeddingProvider;
use weave_graph_store_sqlite::SqliteStorage;

fn node(symbol: &str, path: &str) -> Node {
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

#[test]
fn finds_the_closest_chunk_by_shared_vocabulary() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let embedder = MockEmbeddingProvider::new();
    let id_a = storage
        .upsert_node(&node("check_jwt_expiry", "auth.rs"))
        .unwrap();
    let id_b = storage
        .upsert_node(&node("render_page_layout", "view.rs"))
        .unwrap();
    storage
        .rebuild_vector_index(
            &embedder,
            &[
                (id_a, "verify auth token expiry".to_string()),
                (id_b, "render page layout".to_string()),
            ],
        )
        .unwrap();

    let hits = weave_search_semantic(
        &storage,
        SemanticSearchArgs {
            query: "verify auth token expiry",
            limit: 5,
        },
        None,
    )
    .unwrap();

    assert!(!hits.is_empty());
    assert_eq!(hits[0].symbol, "check_jwt_expiry");
}

/// SEC-01 through the MCP layer: a masked top hit must not shrink a
/// `limit`-capped result set — same guarantee
/// `weave-graph-cli::search::run_semantic` already carries, exercised here
/// through this crate's own wrapper instead of the CLI's.
#[test]
fn a_masked_top_hit_does_not_starve_a_visible_runner_up() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let embedder = MockEmbeddingProvider::new();
    let id_private = storage
        .upsert_node(&node("check_jwt_expiry", "internal/auth.rs"))
        .unwrap();
    let id_public = storage
        .upsert_node(&node("render_page_layout", "view.rs"))
        .unwrap();
    storage
        .rebuild_vector_index(
            &embedder,
            &[
                (id_private, "verify auth token expiry".to_string()),
                (id_public, "render page layout".to_string()),
            ],
        )
        .unwrap();
    let visible: &dyn Fn(&Node) -> bool = &|n| n.path != "internal/auth.rs";

    let hits = weave_search_semantic(
        &storage,
        SemanticSearchArgs {
            query: "verify auth token expiry",
            limit: 1,
        },
        Some(visible),
    )
    .unwrap();

    assert_eq!(hits.len(), 1, "a visible match must still surface");
    assert_eq!(hits[0].symbol, "render_page_layout");
}
