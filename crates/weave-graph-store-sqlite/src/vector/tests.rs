use weave_graph_core::embedding::MockEmbeddingProvider;

use crate::SqliteStorage;

#[test]
fn rebuild_then_search_finds_the_closest_chunk_by_shared_vocabulary() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let embedder = MockEmbeddingProvider::new();
    let chunks = vec![
        (1u32, "fn check_jwt_ttl(token: &str) -> bool".to_string()),
        (
            2u32,
            "fn render_html_layout(page: &Page) -> String".to_string(),
        ),
    ];
    storage.rebuild_vector_index(&embedder, &chunks).unwrap();

    let hits = storage
        .search_vector(&embedder, "verify jwt token expiry", 5, 4)
        .unwrap();

    assert_eq!(hits.first(), Some(&1u32));
}

#[test]
fn rebuild_clears_stale_entries_from_a_previous_rebuild() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let embedder = MockEmbeddingProvider::new();
    storage
        .rebuild_vector_index(&embedder, &[(1, "auth login handler".to_string())])
        .unwrap();
    assert_eq!(
        storage
            .search_vector(&embedder, "auth login", 5, 4)
            .unwrap()
            .len(),
        1
    );

    // A rebuild with a disjoint chunk set must not still surface the old
    // one — `vec_chunks` is a derived index, never its own source of truth.
    storage
        .rebuild_vector_index(&embedder, &[(2, "render html template".to_string())])
        .unwrap();
    let hits = storage
        .search_vector(&embedder, "auth login", 5, 4)
        .unwrap();
    assert!(!hits.contains(&1));
}

#[test]
fn search_respects_the_limit() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let embedder = MockEmbeddingProvider::new();
    let chunks: Vec<_> = (0..5)
        .map(|i| (i, format!("auth handler variant number {i}")))
        .collect();
    storage.rebuild_vector_index(&embedder, &chunks).unwrap();

    let hits = storage
        .search_vector(&embedder, "auth handler", 2, 4)
        .unwrap();
    assert_eq!(hits.len(), 2);
}

#[test]
fn search_against_an_empty_index_returns_no_matches() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let embedder = MockEmbeddingProvider::new();
    assert!(
        storage
            .search_vector(&embedder, "anything", 5, 4)
            .unwrap()
            .is_empty()
    );
}
