//! Recall + latency of the three-stage ANN funnel against brute-force
//! cosine ground truth. Uses the deterministic mock embedder on purpose:
//! this measures the *index*, not a model — BGE-independent by design,
//! so a regression gate can run without any ML asset.

use std::time::Instant;

use criterion::{Criterion, criterion_group, criterion_main};
use weave_graph_core::embedding::{EmbeddingProvider, MockEmbeddingProvider};
use weave_graph_core::ranking::reciprocal_rank_fusion;
use weave_graph_core::{Node, Storage};
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
    let storage = SqliteStorage::open_rebuild(&dir.path().join("bench.db")).unwrap();
    storage.begin_bulk_write().unwrap();
    let embedder = MockEmbeddingProvider::new();
    storage
        .rebuild_vector_index_streaming(&embedder, |insert| {
            for i in 0..n {
                insert(i as u32, &chunk_text(i))?;
            }
            Ok(())
        })
        .unwrap();
    storage.commit_bulk_write().unwrap();
    storage.checkpoint_wal().unwrap();
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

fn run_scale_and_retriever_sweep() {
    let embedder = MockEmbeddingProvider::new();
    for &n in &[1_000usize, 10_000, 50_000, 100_000] {
        let build_start = Instant::now();
        let (dir, storage, queries) = seeded(n);
        let build_ms = build_start.elapsed().as_millis();
        let db_bytes = std::fs::metadata(dir.path().join("bench.db"))
            .map(|m| m.len())
            .unwrap_or(0);
        let corpus: Vec<(u32, Vec<f32>)> = (0..n)
            .map(|i| (i as u32, embedder.embed(&chunk_text(i)).unwrap()))
            .collect();
        let truths: Vec<Vec<u32>> = (0..5)
            .map(|q| {
                brute_force_top_k(
                    &corpus,
                    &embedder.embed_query(&query_text(q)).unwrap(),
                    TOP_K,
                )
            })
            .collect();
        let mut overlap = 0usize;
        let mut latencies_us = Vec::with_capacity(queries.len());
        for &q in &queries {
            let q_start = Instant::now();
            let hits = storage
                .search_vector(&embedder, &query_text(q), TOP_K, 4, None)
                .unwrap();
            latencies_us.push(q_start.elapsed().as_micros());
            let truth = &truths[q % 5];
            overlap += hits.iter().filter(|h| truth.contains(h)).count();
        }
        latencies_us.sort_unstable();
        let p50_us = latencies_us[latencies_us.len() / 2];
        let p95_us = latencies_us[(latencies_us.len() * 95) / 100];
        let recall = overlap as f64 / (queries.len() * TOP_K) as f64;
        eprintln!(
            "scale chunks={n} build_ms={build_ms} db_kib={} p50_us={p50_us} p95_us={p95_us} recall@{TOP_K}={recall:.3}",
            db_bytes / 1024,
        );
    }

    // Compare lexical (FTS5), vector (quantized ANN), and hybrid RRF on mixed
    // identifier + synonym queries where neither single signal wins every query.
    let dir = tempfile::tempdir().unwrap();
    let mut storage = SqliteStorage::open(&dir.path().join("hybrid.db")).unwrap();
    storage.begin_bulk_write().unwrap();
    let nodes: Vec<Node> = (0..CHUNK_COUNT)
        .map(|i| Node {
            id: 0,
            repo_id: "local".into(),
            path: format!("src/mod_{}.rs", i / 20),
            symbol: format!("handler_{i}"),
            kind: "function".into(),
            line_start: 1,
            line_end: 3,
            signature: chunk_text(i),
        })
        .collect();
    let ids = storage.insert_fresh_nodes(&nodes).unwrap();
    for (idx, &id) in ids.iter().enumerate() {
        storage.upsert_fts_text(id, &chunk_text(idx), "").unwrap();
    }
    storage.commit_bulk_write().unwrap();
    storage
        .rebuild_vector_index_streaming(&embedder, |insert| {
            for (idx, &id) in ids.iter().enumerate() {
                insert(id, &chunk_text(idx))?;
            }
            Ok(())
        })
        .unwrap();

    // Ground truth for 10 queries (5 exact handler identifiers, 5 topic queries).
    let mixed_queries: Vec<(String, u32)> = vec![
        ("handler_0".into(), ids[0]),
        ("handler_42".into(), ids[42]),
        ("handler_105".into(), ids[105]),
        ("handler_511".into(), ids[511]),
        ("handler_999".into(), ids[999]),
        ("verify jwt token credential".into(), ids[0]),
        ("render html layout template".into(), ids[1]),
        ("sql repository migration".into(), ids[2]),
        ("event packet delivery queue".into(), ids[3]),
        ("calculate metric evaluation".into(), ids[4]),
    ];
    let (mut fts_hits, mut vec_hits, mut rrf_hits) = (0usize, 0usize, 0usize);
    let (mut fts_us, mut vec_us, mut rrf_us) = (0u128, 0u128, 0u128);
    for (q, expected_topic_mod) in &mixed_queries {
        let t0 = Instant::now();
        let lex = storage.search_symbols(q, TOP_K, None).unwrap();
        let d_lex = t0.elapsed().as_micros();
        fts_us += d_lex;

        let t1 = Instant::now();
        let vec_res = storage.search_vector(&embedder, q, TOP_K, 4, None).unwrap();
        let d_vec = t1.elapsed().as_micros();
        vec_us += d_vec;

        let t2 = Instant::now();
        let rrf = reciprocal_rank_fusion(&[&lex, &vec_res], TOP_K);
        rrf_us += d_lex + d_vec + t2.elapsed().as_micros();

        let matches_target = |list: &[u32]| {
            if q.starts_with("handler_") {
                list.contains(expected_topic_mod)
            } else {
                let want_mod = (*expected_topic_mod - 1) % 5;
                list.iter().any(|&id| (id - 1) % 5 == want_mod)
            }
        };
        fts_hits += usize::from(matches_target(&lex));
        vec_hits += usize::from(matches_target(&vec_res));
        rrf_hits += usize::from(matches_target(&rrf));
    }
    let qn = mixed_queries.len();
    eprintln!(
        "retriever_compare queries={qn} fts_hit_rate={:.2} fts_avg_us={} vec_hit_rate={:.2} vec_avg_us={} rrf_hit_rate={:.2} rrf_avg_us={}",
        fts_hits as f64 / qn as f64,
        fts_us / qn as u128,
        vec_hits as f64 / qn as f64,
        vec_us / qn as u128,
        rrf_hits as f64 / qn as f64,
        rrf_us / qn as u128,
    );
}

fn ann_recall_at_k(c: &mut Criterion) {
    if std::env::var_os("WEAVE_VECTOR_SCALE_SWEEP").is_some_and(|v| v == "1") {
        run_scale_and_retriever_sweep();
    }
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
