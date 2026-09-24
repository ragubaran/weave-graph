use weave_graph_core::embedding::{EmbeddingError, EmbeddingProvider, MockEmbeddingProvider};
use weave_graph_core::{Node, Storage};

use crate::SqliteStorage;

struct AlternateEmbeddingProvider;

impl EmbeddingProvider for AlternateEmbeddingProvider {
    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        MockEmbeddingProvider::new().embed(text)
    }

    fn dimensions(&self) -> usize {
        384
    }

    fn model_id(&self) -> &str {
        "test-alternate-v1"
    }
}

fn node(path: &str) -> Node {
    Node {
        id: 0,
        repo_id: "local".to_string(),
        path: path.to_string(),
        symbol: "s".to_string(),
        kind: "function".to_string(),
        line_start: 1,
        line_end: 2,
        signature: "fn s()".to_string(),
    }
}

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
        .search_vector(&embedder, "verify jwt token expiry", 5, 4, None)
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
            .search_vector(&embedder, "auth login", 5, 4, None)
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
        .search_vector(&embedder, "auth login", 5, 4, None)
        .unwrap();
    assert!(!hits.contains(&1));
}

#[test]
fn purge_vector_paths_removes_rows_by_node_rowid() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let embedder = MockEmbeddingProvider::new();
    let secret_id = storage.upsert_node(&node("secret.rs")).unwrap();
    let public_id = storage.upsert_node(&node("public.rs")).unwrap();
    storage
        .rebuild_vector_index(
            &embedder,
            &[
                (
                    secret_id,
                    "secret authentication implementation".to_string(),
                ),
                (public_id, "public authentication interface".to_string()),
            ],
        )
        .unwrap();

    assert_eq!(
        storage
            .purge_vector_paths(&["secret.rs".to_string()])
            .unwrap(),
        1
    );
    let hits = storage
        .search_vector(&embedder, "authentication", 5, 4, None)
        .unwrap();
    assert!(!hits.contains(&secret_id));
    assert!(hits.contains(&public_id));
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
        .search_vector(&embedder, "auth handler", 2, 4, None)
        .unwrap();
    assert_eq!(hits.len(), 2);
}

#[test]
fn vector_operations_reject_the_wrong_embedding_dimension() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let embedder = MockEmbeddingProvider::with_dimensions(32);

    let err = storage
        .rebuild_vector_index(&embedder, &[(1, "auth handler".to_string())])
        .unwrap_err();

    assert!(err.to_string().contains("do not match index dimensions"));
}

#[test]
fn vector_operations_reject_a_mixed_model_index() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let original = MockEmbeddingProvider::new();
    let alternate = AlternateEmbeddingProvider;
    storage
        .rebuild_vector_index(&original, &[(1, "auth handler".to_string())])
        .unwrap();

    let search_error = storage
        .search_vector(&alternate, "auth handler", 5, 4, None)
        .unwrap_err();
    assert!(search_error.to_string().contains("built with mock-fnv-v1"));

    let update_error = storage
        .upsert_vector_index_streaming(&alternate, |insert| insert(1, "auth handler"))
        .unwrap_err();
    assert!(update_error.to_string().contains("built with mock-fnv-v1"));
}

/// SEC-01: the best-ranked candidate being masked must not shrink the
/// result set — a visible runner-up must still surface, not get starved
/// by a `truncate(limit)` that ran before masking was ever applied.
#[test]
fn search_does_not_starve_a_visible_runner_up_behind_a_masked_top_hit() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let embedder = MockEmbeddingProvider::new();
    let secret_id = storage.upsert_node(&node("secret.rs")).unwrap();
    let public_id = storage.upsert_node(&node("public.rs")).unwrap();
    // `secret`'s chunk is the query text verbatim — deterministically the
    // single best-scoring candidate, so an unmasked search at limit=1
    // would return only `secret_id`. `public`'s chunk shares only some
    // vocabulary, scoring lower but still a real, visible match.
    storage
        .rebuild_vector_index(
            &embedder,
            &[
                (secret_id, "auth handler exact match".to_string()),
                (public_id, "auth handler unrelated other words".to_string()),
            ],
        )
        .unwrap();

    let hide_secret = |n: &Node| n.path != "secret.rs";
    let hits = storage
        .search_vector(
            &embedder,
            "auth handler exact match",
            1,
            4,
            Some(&hide_secret),
        )
        .unwrap();
    assert_eq!(
        hits,
        vec![public_id],
        "the visible node must survive even though it may rank second"
    );
}

#[test]
fn search_against_an_empty_index_returns_no_matches() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let embedder = MockEmbeddingProvider::new();
    assert!(
        storage
            .search_vector(&embedder, "anything", 5, 4, None)
            .unwrap()
            .is_empty()
    );
}

/// POL-02's empirical scale check: two chunks embedding the *same* text
/// are, after binary+int8 quantization round-tripping, as close to
/// identical as this pipeline can represent — this is what proves
/// `INT8_UNIT_SCALE`'s `127*127` normalization actually lands near `1.0`
/// for a real near-duplicate, not just a number chosen to look plausible.
#[test]
fn find_similar_node_pairs_scores_a_near_duplicate_close_to_one() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let embedder = MockEmbeddingProvider::new();
    storage
        .rebuild_vector_index(
            &embedder,
            &[
                (1, "fn check_jwt_ttl(token: &str) -> bool".to_string()),
                (2, "fn check_jwt_ttl(token: &str) -> bool".to_string()),
                (
                    3,
                    "fn render_html_layout(page: &Page) -> String".to_string(),
                ),
            ],
        )
        .unwrap();

    let pairs = storage.find_similar_node_pairs(&[1, 2, 3], 0.0, 8).unwrap();
    let duplicate_pair = pairs
        .iter()
        .find(|(a, b, _)| (*a, *b) == (1, 2))
        .unwrap_or_else(|| panic!("expected (1, 2) in {pairs:?}"));
    assert!(
        duplicate_pair.2 > 0.9,
        "near-duplicate chunks should score close to 1.0, got {duplicate_pair:?}"
    );
}

#[test]
fn find_similar_node_pairs_dedupes_a_b_and_b_a_into_one_entry() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let embedder = MockEmbeddingProvider::new();
    storage
        .rebuild_vector_index(
            &embedder,
            &[
                (1, "fn check_jwt_ttl(token: &str) -> bool".to_string()),
                (2, "fn check_jwt_ttl(token: &str) -> bool".to_string()),
            ],
        )
        .unwrap();

    let pairs = storage.find_similar_node_pairs(&[1, 2], 0.0, 8).unwrap();
    let matching: Vec<_> = pairs
        .iter()
        .filter(|(a, b, _)| (*a, *b) == (1, 2) || (*a, *b) == (2, 1))
        .collect();
    assert_eq!(matching.len(), 1, "{pairs:?}");
}

#[test]
fn find_similar_node_pairs_excludes_scores_below_threshold() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    let embedder = MockEmbeddingProvider::new();
    storage
        .rebuild_vector_index(
            &embedder,
            &[
                (1, "fn check_jwt_ttl(token: &str) -> bool".to_string()),
                (
                    2,
                    "fn render_html_layout(page: &Page) -> String".to_string(),
                ),
            ],
        )
        .unwrap();

    let pairs = storage.find_similar_node_pairs(&[1, 2], 0.99, 8).unwrap();
    assert!(
        pairs.is_empty(),
        "unrelated chunks must not pass a 0.99 threshold: {pairs:?}"
    );
}

#[test]
fn find_similar_node_pairs_against_an_empty_index_returns_no_matches() {
    let storage = SqliteStorage::open_in_memory().unwrap();
    assert!(
        storage
            .find_similar_node_pairs(&[1, 2], 0.5, 8)
            .unwrap()
            .is_empty()
    );
}
