//! Core Invariant 4's "500k symbols < 80MB peak RSS" check: builds 500k
//! real nodes + edges through a real SQLite DB, then loads a `CsrGraph`
//! from it — the full path an actual `weave index` takes, not just the
//! CSR's own analytical byte layout (see `csr_memory.rs`).
//!
//! Self-measures peak RSS via `getrusage(RUSAGE_SELF)` and exits non-zero
//! above the 80 MB ceiling, so CI can gate on it directly:
//!   cargo run --release -p weave-graph-store-sqlite --example mem_500k

use weave_graph_core::{CsrGraph, Edge, Node, Storage};
use weave_graph_store_sqlite::SqliteStorage;

const SYMBOL_COUNT: u32 = 500_000;
/// Core Invariant 4's ceiling, in bytes.
const PEAK_RSS_BUDGET: u64 = 80 * 1024 * 1024;

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn peak_rss_bytes() -> u64 {
    let mut usage = std::mem::MaybeUninit::uninit();
    // Safety: `usage` is a correctly-sized out-pointer per getrusage(2);
    // a zero return means the kernel fully populated it.
    let ret = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if ret != 0 {
        return 0;
    }
    let usage = unsafe { usage.assume_init() };
    // macOS reports ru_maxrss in bytes; Linux in kilobytes.
    #[cfg(target_os = "macos")]
    {
        usage.ru_maxrss as u64
    }
    #[cfg(target_os = "linux")]
    {
        usage.ru_maxrss as u64 * 1024
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn peak_rss_bytes() -> u64 {
    0
}

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
    let peak = peak_rss_bytes();
    let peak_mib = peak / (1024 * 1024);
    println!(
        "loaded {} nodes, {} edges into CsrGraph; peak RSS {peak_mib} MiB (budget {} MiB)",
        csr.node_count(),
        csr.edge_count(),
        PEAK_RSS_BUDGET / (1024 * 1024),
    );
    if peak > PEAK_RSS_BUDGET {
        eprintln!("FAIL: peak RSS {peak_mib} MiB exceeds the 80 MiB envelope");
        std::process::exit(1);
    }
}
