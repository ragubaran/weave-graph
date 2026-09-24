//! Point lookup, 3-hop traversal, and
//! batch-insert throughput on `rusqlite` alone — the `libSQL` comparison
//! lives in the `turso` feature's own bench.

use criterion::{Criterion, criterion_group, criterion_main};
use weave_graph_core::{Edge, Node, Storage};
use weave_graph_store_sqlite::SqliteStorage;

fn node(i: u32) -> Node {
    Node {
        id: 0,
        repo_id: "r".into(),
        path: format!("f{i}.rs"),
        symbol: format!("s{i}"),
        kind: "function".into(),
        line_start: 1,
        line_end: 2,
        signature: format!("fn s{i}()"),
    }
}

/// A chain DB `0 -> 1 -> 2 -> ... -> n-1` — enough hops for a real 3-hop
/// traversal, seeded once and reused read-only across the lookup/traversal
/// groups so their timings don't include insert cost.
fn seeded_chain(n: u32) -> (tempfile::TempDir, SqliteStorage, Vec<u32>) {
    let dir = tempfile::tempdir().unwrap();
    let mut storage = SqliteStorage::open(&dir.path().join("bench.db")).unwrap();
    let mut ids = Vec::with_capacity(n as usize);
    for i in 0..n {
        ids.push(storage.upsert_node(&node(i)).unwrap());
    }
    for w in ids.windows(2) {
        storage
            .upsert_edge(&Edge {
                id: 0,
                source_id: w[0],
                target_id: w[1],
                kind: "CALLS_EXACT".into(),
                weight: 1.0,
                extractor: None,
                resolution_kind: None,
            })
            .unwrap();
    }
    (dir, storage, ids)
}

fn point_lookup(c: &mut Criterion) {
    let (_dir, storage, ids) = seeded_chain(10_000);
    let mid = ids[ids.len() / 2];
    c.bench_function("sqlite/get_node", |b| {
        b.iter(|| storage.get_node(mid).unwrap());
    });
}

fn three_hop_traversal(c: &mut Criterion) {
    let (_dir, storage, ids) = seeded_chain(10_000);
    let from = ids[0];
    let to = ids[3];
    c.bench_function("sqlite/query_path_3_hops", |b| {
        b.iter(|| storage.query_path(from, to).unwrap());
    });
}

fn batch_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("sqlite/batch_insert");
    for size in [1_000u32, 10_000] {
        group.bench_with_input(
            criterion::BenchmarkId::new("nodes", size),
            &size,
            |b, &size| {
                b.iter_batched(
                    || tempfile::tempdir().unwrap(),
                    |dir| {
                        let mut storage =
                            SqliteStorage::open(&dir.path().join("bench.db")).unwrap();
                        // One transaction for the whole batch, matching
                        // `weave index`'s own write path — per-statement
                        // autocommit was the measured root cause of the
                        // Core Invariant 4 violation.
                        storage.begin_bulk_write().unwrap();
                        for i in 0..size {
                            storage.upsert_node(&node(i)).unwrap();
                        }
                        storage.commit_bulk_write().unwrap();
                    },
                    criterion::BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

criterion_group!(benches, point_lookup, three_hop_traversal, batch_insert);
criterion_main!(benches);
