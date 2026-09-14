//! Recall + latency of the three-stage ANN funnel against brute-force
//! cosine ground truth. Uses the deterministic mock embedder on purpose:
//! this measures the *index*, not a model — BGE-independent by design,
//! so a regression gate can run without any ML asset.

use std::time::Instant;

use criterion::{Criterion, criterion_group, criterion_main};
use weave_graph_core::Storage;
use weave_graph_core::embedding::{EmbeddingProvider, MockEmbeddingProvider};
use weave_graph_store_sqlite::SqliteStorage;

const CHUNK_COUNT: usize = 2_000;
const QUERY_COUNT: usize = 50;
const TOP_K: usize = 10;

/// A vocabulary-poor synthetic corpus: many near-duplicate chunks around a
/// few topics, so ranking quality (not just existence) is actually tested.
fn chunk_text(i: usize) -> String {
    let topics = [
        "auth token expiry verify jwt credential login",
        "render html template layout page component",
        "database store repository sql query migration",
        "message payload event packet queue delivery",
        "calculate score evaluate metric compute eval",
    ];
    let topic = topics[i % topics.len()];
    format!("fn handler_{i}() {{ {topic} variant {i} }}")
}

fn query_text(q: usize) -> String {
    let queries = [
        "verify token lifetime",
        "page layout render",
        "database migration query",
        "event delivery queue",
        "compute evaluation score",
    ];
    queries[q % queries.len()].to_string()
}

fn seeded(n: usize) -> (tempfile::TempDir, SqliteStorage, Vec<usize>) {
    let dir = tempfile::tempdir().unwrap();
    let storage = SqliteStorage::open(&dir.path().join("bench.db")).unwrap();
    let embedder = MockEmbeddingProvider::new();
    let chunks: Vec<(u32, String)> = (0..n).map(|i| (i as u32, chunk_text(i))).collect();
    storage.rebuild_vector_index(&embedder, &chunks).unwrap();
    let queries: Vec<usize> = (0..QUERY_COUNT).collect();
    (dir, storage, queries)
}

/// Brute-force cosine ground truth over the same float32 embeddings.
fn brute_force_top_k(corpus: &[(u32, Vec<f32>)], query: &[f32], k: usize) -> Vec<u32> {
    let mut scored: Vec<(u32, f32)> = corpus
        .iter()
        .map(|(id, vec)| (*id, query.iter().zip(vec).map(|(a, b)| a * b).sum()))
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    scored.truncate(k);
    scored.into_iter().map(|(id, _)| id).collect()
}

fn ann_recall_at_k(c: &mut Criterion) {
    let (dir, storage, queries) = seeded(CHUNK_COUNT);
    let embedder = MockEmbeddingProvider::new();
    let corpus: Vec<(u32, Vec<f32>)> = (0..CHUNK_COUNT)
        .map(|i| (i as u32, embedder.embed(&chunk_text(i)).unwrap()))
        .collect();

    let mut total_overlap = 0usize;
    let start = Instant::now();
    for &q in &queries {
        let hits = storage
            .search_vector(&embedder, &query_text(q), TOP_K, 4, None)
            .unwrap();
        let truth = brute_force_top_k(
            &corpus,
            &embedder.embed_query(&query_text(q)).unwrap(),
            TOP_K,
        );
        total_overlap += hits.iter().filter(|h| truth.contains(h)).count();
    }
    let elapsed = start.elapsed();
    let recall = total_overlap as f64 / (queries.len() * TOP_K) as f64;
    drop(dir);
    eprintln!(
        "recall@{TOP_K} = {recall:.3} over {QUERY_COUNT} queries, avg {:.2?}/query",
        elapsed / queries.len() as u32,
    );

    let (_dir, storage, queries) = seeded(CHUNK_COUNT);
    let embedder = MockEmbeddingProvider::new();
    c.bench_function("vector/ann_search_top10", |b| {
        b.iter(|| {
            for &q in &queries {
                let _ = storage
                    .search_vector(&embedder, &query_text(q), TOP_K, 4, None)
                    .unwrap();
            }
        })
    });
}

criterion_group!(benches, ann_recall_at_k);
criterion_main!(benches);
