//! `impl.md` M2.7's bench: the libSQL side of the `rusqlite`-vs-`libSQL`
//! batch-insert comparison M1.9 deferred (`performance_compare.md`
//! §5.2.3), mirroring `benches/sqlite_latency.rs`'s methodology exactly.
//!
//! It cannot live in the sqlite crate's `sqlite_latency.rs`: rusqlite's
//! bundled `libsqlite3-sys` and libSQL's `libsql-ffi` both statically
//! define the SQLite C symbols, so one benchmark binary cannot link both
//! backends. The comparison is therefore between two criterion targets —
//! record both `batch_insert` groups side by side when reporting.

use criterion::{Criterion, criterion_group, criterion_main};
use weave_graph_core::{Node, Storage};
use weave_graph_store_turso::TursoStorage;

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

fn batch_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("turso/batch_insert");
    for size in [1_000u32, 10_000] {
        group.bench_with_input(
            criterion::BenchmarkId::new("nodes", size),
            &size,
            |b, &size| {
                b.iter_batched(
                    || tempfile::tempdir().unwrap(),
                    |dir| {
                        let mut storage = TursoStorage::open(&dir.path().join("bench.db")).unwrap();
                        // One transaction for the whole batch, matching
                        // the sqlite bench's write path — per-statement
                        // autocommit was M1.9's measured bottleneck.
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

criterion_group!(benches, batch_insert);
criterion_main!(benches);
