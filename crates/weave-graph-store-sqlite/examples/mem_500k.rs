//! `impl.md` M1.9's "Core RAM (500k symbols) < 80MB" registry row: builds
//! 500k real nodes + edges through a real SQLite DB, then loads a
//! `CsrGraph` from it — the full path an actual `weave index` takes, not
//! just the CSR's own analytical byte layout (see `csr_memory.rs`).
//! Measure peak RSS externally, e.g.:
//!   /usr/bin/time -l cargo run --release -p weave-graph-store-sqlite --example mem_500k

use weave_graph_core::{CsrGraph, Edge, Node, Storage};
use weave_graph_store_sqlite::SqliteStorage;

const SYMBOL_COUNT: u32 = 500_000;

fn main() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut storage = SqliteStorage::open(&dir.path().join("mem_500k.db")).expect("open db");

    let mut ids = Vec::with_capacity(SYMBOL_COUNT as usize);
    storage.begin_bulk_write().expect("begin bulk write");
    for i in 0..SYMBOL_COUNT {
        let node = Node {
            id: 0,
            repo_id: "r".into(),
            path: format!("f{}.rs", i / 20),
            symbol: format!("s{i}"),
            kind: "function".into(),
            line_start: 1,
            line_end: 2,
            signature: format!("fn s{i}()"),
        };
        ids.push(storage.upsert_node(&node).expect("upsert node"));
    }
    for w in ids.windows(2) {
        let edge = Edge {
            id: 0,
            source_id: w[0],
            target_id: w[1],
            kind: "CALLS_EXACT".into(),
            weight: 1.0,
        };
        storage.upsert_edge(&edge).expect("upsert edge");
    }
    storage.commit_bulk_write().expect("commit bulk write");

    let csr = CsrGraph::load(&storage).expect("load csr");
    println!(
        "loaded {} nodes, {} edges into CsrGraph",
        csr.node_count(),
        csr.edge_count()
    );
}
