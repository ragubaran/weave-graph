//! Builds 500k real nodes and edges through Turso, then loads `CsrGraph`.
//! Peak RSS covers every phase and fails above the 80 MiB envelope.

use std::time::Instant;
use weave_graph_core::{CsrGraph, Edge, Node, Storage};
use weave_graph_store_turso::TursoStorage;

const SYMBOL_COUNT: u32 = 500_000;
const WRITE_BATCH_ROWS: usize = 5_000;
const PEAK_RSS_BUDGET: u64 = 80 * 1024 * 1024;

#[cfg(target_os = "macos")]
fn peak_rss_bytes() -> u64 {
    let mut usage = std::mem::MaybeUninit::uninit();
    // Safety: getrusage(2) is a standard POSIX syscall; `usage` is a
    // correctly-sized out-pointer and the kernel fully populates it on success.
    let ret = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if ret != 0 {
        return 0;
    }
    let usage = unsafe { usage.assume_init() };
    usage.ru_maxrss as u64
}

#[cfg(target_os = "linux")]
fn peak_rss_bytes() -> u64 {
    let mut usage = std::mem::MaybeUninit::uninit();
    // Safety: getrusage(2) is a standard POSIX syscall; `usage` is a
    // correctly-sized out-pointer and the kernel fully populates it on success.
    let ret = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if ret != 0 {
        return 0;
    }
    let usage = unsafe { usage.assume_init() };
    usage.ru_maxrss as u64 * 1024
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
    let mut storage = TursoStorage::open(&dir.path().join("mem_500k.db")).expect("open db");

    let mut ids = Vec::with_capacity(SYMBOL_COUNT as usize);
    let start = Instant::now();
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
        if i % WRITE_BATCH_ROWS as u32 == 0 {
            storage.begin_bulk_write().expect("begin node batch");
        }
        let id = storage.upsert_node(&node).expect("upsert node");
        ids.push(id);
        if (i + 1) % WRITE_BATCH_ROWS as u32 == 0 || i + 1 == SYMBOL_COUNT {
            storage.commit_bulk_write().expect("commit node batch");
        }
    }
    let elapsed = start.elapsed();
    eprintln!(
        "inserted {} nodes in {:.2}s",
        SYMBOL_COUNT,
        elapsed.as_secs_f64()
    );
    report_peak("nodes");

    let start = Instant::now();
    for i in 0..(ids.len() as u32).saturating_sub(1) {
        let edge = Edge {
            id: 0,
            source_id: ids[i as usize],
            target_id: ids[(i + 1) as usize],
            kind: "CALLS_EXACT".into(),
            weight: 1.0,
        };
        if i % WRITE_BATCH_ROWS as u32 == 0 {
            storage.begin_bulk_write().expect("begin edge batch");
        }
        storage.upsert_edge(&edge).expect("upsert edge");
        if (i + 1) % WRITE_BATCH_ROWS as u32 == 0 || i + 1 == ids.len() as u32 - 1 {
            storage.commit_bulk_write().expect("commit edge batch");
        }
    }
    let elapsed = start.elapsed();
    eprintln!(
        "inserted {} edges in {:.2}s",
        ids.len() - 1,
        elapsed.as_secs_f64()
    );
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
