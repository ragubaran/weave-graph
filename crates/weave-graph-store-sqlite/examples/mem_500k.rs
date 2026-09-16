//! Builds 500k real nodes and edges through SQLite, then loads `CsrGraph`.
//! Staged write checkpoints match the bounded rebuild publication path.
//! Peak RSS covers every phase and fails above the 80 MiB envelope.

use weave_graph_core::{CsrGraph, Edge, Node};
use weave_graph_store_sqlite::SqliteStorage;

const SYMBOL_COUNT: u32 = 500_000;
const WRITE_BATCH_ROWS: usize = 10_000;
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

fn report_peak(phase: &str) -> u64 {
    let peak = peak_rss_bytes();
    eprintln!("phase={phase} peak_rss_mib={}", peak / (1024 * 1024));
    peak
}

fn main() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut storage = SqliteStorage::open(&dir.path().join("mem_500k.db")).expect("open db");

    let mut ids = Vec::with_capacity(SYMBOL_COUNT as usize);
    for start in (0..SYMBOL_COUNT).step_by(WRITE_BATCH_ROWS) {
        let end = (start + WRITE_BATCH_ROWS as u32).min(SYMBOL_COUNT);
        let nodes = (start..end)
            .map(|i| Node {
                id: 0,
                repo_id: "r".into(),
                path: format!("f{}.rs", i / 20),
                symbol: format!("s{i}"),
                kind: "function".into(),
                line_start: 1,
                line_end: 2,
                signature: format!("fn s{i}()"),
            })
            .collect::<Vec<_>>();
        storage.begin_bulk_write().expect("begin node batch");
        ids.extend(
            storage
                .insert_fresh_nodes(&nodes)
                .expect("insert node batch"),
        );
        storage.commit_bulk_write().expect("commit node batch");
        storage.checkpoint_wal().expect("checkpoint node batch");
    }
    report_peak("nodes");

    for start in (0..ids.len().saturating_sub(1)).step_by(WRITE_BATCH_ROWS) {
        let end = (start + WRITE_BATCH_ROWS + 1).min(ids.len());
        let edges = ids[start..end]
            .windows(2)
            .map(|pair| Edge {
                id: 0,
                source_id: pair[0],
                target_id: pair[1],
                kind: "CALLS_EXACT".into(),
                weight: 1.0,
            })
            .collect::<Vec<_>>();
        storage.begin_bulk_write().expect("begin edge batch");
        storage.upsert_edges(&edges).expect("upsert edge batch");
        storage.commit_bulk_write().expect("commit edge batch");
        storage.checkpoint_wal().expect("checkpoint edge batch");
    }
    report_peak("edges");

    let csr = CsrGraph::load(&storage).expect("load csr");
    let peak = report_peak("csr");
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
