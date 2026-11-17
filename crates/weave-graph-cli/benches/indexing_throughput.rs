//! Indexing write-path throughput over a pinned synthetic corpus: the
//! two-pass parallel parse → insert-fresh-nodes → ProjectIndex → resolve-edges
//! → recreate-indexes → bulk-commit sequence a full `weave index` runs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Instant;

use criterion::{Criterion, criterion_group, criterion_main};
use rayon::{ThreadPoolBuilder, prelude::*};

use weave_graph_core::{Edge, Node, NodeId, Storage};
use weave_graph_parse::{ParsedFile, ProjectIndex, parse_file};
use weave_graph_store_sqlite::SqliteStorage;

const FILE_COUNT: usize = 500;
const SYMBOLS_PER_FILE: usize = 10;
const PARSE_CHUNK: usize = 32;

/// Deterministic synthetic corpus: `FILE_COUNT` Rust files, each with
/// `SYMBOLS_PER_FILE` functions, half of them calling a previous symbol.
fn write_corpus(root: &Path) -> Vec<PathBuf> {
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

fn parse_files_overlapped<T: Send>(
    files: &[PathBuf],
    map: impl Fn(&str, &ParsedFile) -> T + Sync + Send,
    mut fold: impl FnMut(&str, T),
) {
    let workers = std::thread::available_parallelism().map_or(1, usize::from);
    let pool = ThreadPoolBuilder::new()
        .num_threads(workers)
        .build()
        .unwrap();
    let (tx, rx) = mpsc::sync_channel::<Vec<(String, Option<T>)>>(1);
    std::thread::scope(|scope| {
        scope.spawn(move || {
            for chunk in files.chunks(PARSE_CHUNK) {
                let mapped = pool.install(|| {
                    chunk
                        .par_iter()
                        .map(|file| {
                            let rel = file.file_name().unwrap().to_string_lossy().into_owned();
                            let source = std::fs::read_to_string(file).unwrap();
                            let out = match parse_file(Path::new(&rel), &source) {
                                Some(Ok(parsed)) => Some(map(&rel, &parsed)),
                                _ => None,
                            };
                            (rel, out)
                        })
                        .collect::<Vec<_>>()
                });
                if tx.send(mapped).is_err() {
                    break;
                }
            }
        });
        for mapped in &rx {
            for (rel, item) in mapped {
                if let Some(item) = item {
                    fold(&rel, item);
                }
            }
        }
    });
}

fn index_corpus(db_path: &Path, files: &[PathBuf]) -> usize {
    let mut storage = SqliteStorage::open_rebuild(db_path).unwrap();
    storage.begin_bulk_write().unwrap();
    let deferred = storage
        .drop_non_unique_indexes(&["nodes", "edges"])
        .unwrap();
    let mut symbols = 0usize;

    // Pass 1: parallel chunked parse + single-writer fresh node insert.
    parse_files_overlapped(files, nodes_of, |_rel, nodes| {
        symbols += storage.insert_fresh_nodes(&nodes).unwrap().len();
    });
    storage.commit_bulk_write().unwrap();
    storage.checkpoint_wal().unwrap();
    storage.begin_bulk_write().unwrap();

    // Build ProjectIndex from storage and run Pass 2 edge resolution.
    let symbol_count = storage.node_count().unwrap();
    let mut project_index = ProjectIndex::with_capacity(symbol_count);
    let mut moniker_to_node_id = HashMap::with_capacity(symbol_count);
    storage
        .for_each_node(&mut |node| {
            let moniker = weave_graph_parse::moniker::build(&node.path, &node.symbol);
            let short = node.symbol.rsplit("::").next().unwrap_or(&node.symbol);
            let mid = project_index.add_symbol(&moniker, short);
            moniker_to_node_id.insert(mid, node.id);
        })
        .unwrap();

    parse_files_overlapped(
        files,
        |_rel, parsed| {
            let (edges, _) = project_index.resolve_ids(parsed);
            edges
                .into_iter()
                .filter_map(|edge| {
                    let src_id: NodeId = *moniker_to_node_id.get(&edge.source_id)?;
                    let tgt_id: NodeId = *moniker_to_node_id.get(&edge.target_id)?;
                    Some(Edge {
                        id: 0,
                        source_id: src_id,
                        target_id: tgt_id,
                        kind: edge.kind,
                        weight: 1.0,
                        extractor: edge.extractor,
                        resolution_kind: Some(edge.resolution_kind.as_str().to_string()),
                    })
                })
                .collect::<Vec<_>>()
        },
        |_rel, resolved| {
            storage.upsert_edges(&resolved).unwrap();
        },
    );

    storage.recreate_indexes(&deferred).unwrap();
    storage.commit_bulk_write().unwrap();
    storage.checkpoint_wal().unwrap();
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
