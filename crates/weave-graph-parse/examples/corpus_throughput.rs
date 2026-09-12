//! One-shot corpus throughput measurement — the companion to
//! `parser_throughput.rs`'s criterion cases. Criterion measures per-file
//! distributions but its per-case setup cost makes a 3,000+-file corpus
//! (e.g. `google/guava`) take hours; this example walks a corpus directory
//! once and reports aggregate MB/s over every file the parser accepts.
//!
//! Usage: `cargo run --release -p weave-graph-parse --example
//! corpus_throughput -- /path/to/corpus [.java] [passes]`

use std::path::{Path, PathBuf};
use std::time::Instant;

use weave_graph_parse::parse_file;

fn walk_files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk_files(&path, ext));
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            out.push(path);
        }
    }
    out
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args
        .next()
        .expect("usage: corpus_throughput <dir> [ext] [passes]");
    let ext = args.next().unwrap_or_else(|| "rs".to_string());
    let passes: u32 = args.next().and_then(|p| p.parse().ok()).unwrap_or(3);

    let files = walk_files(Path::new(&dir), &ext);
    let sources: Vec<(PathBuf, String)> = files
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok().map(|s| (p.clone(), s)))
        .collect();
    let total_bytes: u64 = sources.iter().map(|(_, s)| s.len() as u64).sum();
    println!(
        "corpus: {} files, {:.1} MB, ext=.{ext}, passes={passes}",
        sources.len(),
        total_bytes as f64 / 1_048_576.0
    );

    let mut best = f64::MAX;
    for pass in 1..=passes {
        let start = Instant::now();
        let mut parsed = 0u64;
        for (path, source) in &sources {
            let rel = path
                .strip_prefix(&dir)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();
            if parse_file(Path::new(&rel), source).is_some() {
                parsed += 1;
            }
        }
        let elapsed = start.elapsed().as_secs_f64();
        let throughput = (total_bytes as f64 / 1_048_576.0) / elapsed;
        best = best.min(throughput);
        println!(
            "pass {pass}: {elapsed:.2}s, {parsed}/{total} parsed, {throughput:.1} MB/s",
            total = sources.len()
        );
    }
    println!("best: {best:.1} MB/s");
}
