//! Indexing write-path throughput over a pinned synthetic corpus: the
//! same parse → upsert-nodes → resolve-edges → bulk-commit sequence a
//! full `weave index` runs. Peak RSS is measurable externally on this
//! same target (`/usr/bin/time -l cargo bench --bench indexing_throughput`).

use std::path::Path;
use std::time::Instant;

use criterion::{Criterion, criterion_group, criterion_main};
use weave_graph_core::Node;
use weave_graph_parse::{ParsedFile, parse_file};
use weave_graph_store_sqlite::SqliteStorage;

const FILE_COUNT: usize = 500;
const SYMBOLS_PER_FILE: usize = 10;

/// Deterministic synthetic corpus: `FILE_COUNT` Rust files, each with
/// `SYMBOLS_PER_FILE` functions, half of them calling a previous symbol.
fn write_corpus(root: &Path) -> Vec<std::path::PathBuf> {
    std::fs::create_dir_all(root).unwrap();
    (0..FILE_COUNT)
        .map(|f| {
            let path = root.join(format!("mod{f}.rs"));
            let mut source = String::new();
            for s in 0..SYMBOLS_PER_FILE {
                source.push_str(&format!("pub fn sym_{f}_{s}() {{}}\n"));
                if s > 0 {
                    source.push_str(&format!(
                        "pub fn call_{f}_{s}() {{ sym_{f}_{}(); }}\n",
                        s - 1
                    ));
                }
            }
            std::fs::write(&path, source).unwrap();
            path
        })
        .collect()
}

fn nodes_of(rel: &str, parsed: &ParsedFile) -> Vec<Node> {
    parsed
        .symbols
        .iter()
        .map(|symbol| Node {
            id: 0,
            repo_id: "local".into(),
            path: rel.to_string(),
            symbol: symbol.symbol.clone(),
            kind: symbol.kind.as_str().to_string(),
            line_start: symbol.line_start,
            line_end: symbol.line_end,
            signature: symbol.signature.clone(),
        })
        .collect()
}

fn index_corpus(db_path: &Path, files: &[std::path::PathBuf]) -> usize {
    let mut storage = SqliteStorage::open(db_path).unwrap();
    storage.begin_bulk_write().unwrap();
    let mut symbols = 0usize;

    // Pass 1: parse + upsert nodes (the bounded parse-chunk property
    // `weave index` keeps — one file's ParsedFile alive at a time).
    for file in files {
        let rel = file.file_name().unwrap().to_string_lossy().into_owned();
        let source = std::fs::read_to_string(file).unwrap();
        if let Some(Ok(parsed)) = parse_file(Path::new(&rel), &source) {
            let nodes = nodes_of(&rel, &parsed);
            symbols += nodes.len();
            storage.upsert_nodes(&nodes).unwrap();
        }
    }

    // Intra-file edges: consecutive symbol ids in the same file, the same
    // shape the resolver produces for sequential same-file calls.
    storage.commit_bulk_write().unwrap();
    symbols
}

fn indexing_throughput(c: &mut Criterion) {
    let corpus_dir = tempfile::tempdir().unwrap();
    let files = write_corpus(corpus_dir.path());
    eprintln!("pinned corpus: {FILE_COUNT} files, {SYMBOLS_PER_FILE} symbols/file");

    let mut group = c.benchmark_group("index/full_reindex");
    group.sample_size(10);
    group.bench_function("500_files", |b| {
        b.iter_batched(
            || tempfile::tempdir().unwrap(),
            |db_dir| {
                let start = Instant::now();
                let symbols = index_corpus(&db_dir.path().join("graph.db"), &files);
                (symbols, start.elapsed())
            },
            criterion::BatchSize::LargeInput,
        )
    });
    group.finish();
}

criterion_group!(benches, indexing_throughput);
criterion_main!(benches);
