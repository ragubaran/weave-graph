use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use weave_graph_core::{Edge, Node, NodeId, Storage, StorageError};
use weave_graph_parse::{ParsedFile, ProjectIndex, moniker, parse_file};
use weave_graph_store_sqlite::SqliteStorage;

pub(crate) struct IndexStats {
    pub(crate) files: usize,
    pub(crate) symbols: usize,
    pub(crate) edges: usize,
}

/// Distinct file paths already in `active_db` — the `total_indexed` input
/// `should_bail_out` needs to decide full-vs-incremental.
pub(crate) fn indexed_file_count(active_db: &Path) -> Result<usize, StorageError> {
    let storage = SqliteStorage::open(active_db)?;
    let mut paths = std::collections::HashSet::new();
    storage.for_each_node(&mut |n| {
        paths.insert(n.path);
    })?;
    Ok(paths.len())
}

pub(crate) fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

/// Parses every discoverable file under `root`. Shared by both reindex
/// paths — parsing is cheap (microseconds per file, per `parser_throughput`
/// benches); what `should_bail_out` guards against is SQL write volume on
/// the live database, not CPU time re-deriving `ProjectIndex`.
///
/// Parses under the file's **relative** path, not its absolute one: monikers
/// (`path#qualified.symbol`) are built from whatever path is passed here,
/// and `full_moniker_map` reconstructs those same monikers later from the
/// relative `Node.path` column — an absolute path here would never match.
pub(crate) fn build_project_index(root: &Path, files: &[PathBuf]) -> ProjectIndex {
    let mut project_index = ProjectIndex::new();
    let _ = parse_files_bounded(root, files, |_rel, parsed| {
        project_index.add_file(parsed);
        Ok(())
    });
    project_index
}

/// Files parsed concurrently — the parallel-parse bound (L10, `plan.md`
/// §1.2a). Each in-flight `ParsedFile` is KBs, so the worst-case working
/// set stays orders of magnitude under Invariant 4's 80 MB ceiling: the
/// streaming property PERF-06 demanded, kept by construction, not by hope.
const PARSE_CHUNK: usize = 32;

/// The parse stage is the *only* parallel stage: chunks of files are
/// parsed across the rayon pool, then folded strictly serially into the
/// caller's closure — node/edge writes stay funnel-shaped through the one
/// SQLite connection (L10's single-writer constraint). A parse error or
/// unparseable file is reported and skipped, exactly as the sequential
/// loop always behaved.
fn parse_files_bounded(
    root: &Path,
    files: &[PathBuf],
    mut fold: impl FnMut(&str, &ParsedFile) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    for batch in files.chunks(PARSE_CHUNK) {
        let parsed: Vec<(String, ParsedFile)> = batch
            .par_iter()
            .filter_map(|file_path| {
                let source = fs::read_to_string(file_path).ok()?;
                let rel = rel_path(root, file_path);
                match parse_file(Path::new(&rel), &source) {
                    Some(Ok(parsed)) => Some((rel, parsed)),
                    Some(Err(err)) => {
                        eprintln!("skipping {}: {err}", file_path.display());
                        None
                    }
                    None => None,
                }
            })
            .collect();
        for (rel, parsed) in &parsed {
            fold(rel, parsed)?;
        }
    }
    Ok(())
}

// Parses all files in memory for contract hashing or federated queries
// where global cross-repo AST inspection is explicitly requested. Its only
// callers are the `federation`-feature modules, so the function is
// feature-gated to keep the default build's dead-code lint clean.
#[cfg(feature = "federation")]
pub(crate) fn parse_all(
    root: &Path,
    files: &[PathBuf],
) -> (ProjectIndex, Vec<(PathBuf, ParsedFile)>) {
    let mut project_index = ProjectIndex::new();
    let mut parsed_files = Vec::new();
    for file_path in files {
        if let Ok(source) = fs::read_to_string(file_path) {
            let rel = rel_path(root, file_path);
            match parse_file(Path::new(&rel), &source) {
                Some(Ok(parsed)) => {
                    project_index.add_file(&parsed);
                    parsed_files.push((file_path.clone(), parsed));
                }
                Some(Err(err)) => eprintln!("skipping {}: {err}", file_path.display()),
                None => {}
            }
        }
    }
    (project_index, parsed_files)
}

fn upsert_all_nodes(
    storage: &mut SqliteStorage,
    root: &Path,
    files: &[PathBuf],
) -> Result<usize, StorageError> {
    let mut total_symbols = 0usize;
    parse_files_bounded(root, files, |rel, parsed| {
        for symbol in &parsed.symbols {
            let node = Node {
                id: 0,
                repo_id: "local".to_string(),
                path: rel.to_string(),
                symbol: symbol.symbol.clone(),
                kind: symbol.kind.as_str().to_string(),
                line_start: symbol.line_start,
                line_end: symbol.line_end,
                signature: symbol.signature.clone(),
            };
            storage.upsert_node(&node)?;
            total_symbols += 1;
        }
        Ok(())
    })?;
    Ok(total_symbols)
}

/// Every currently-stored node's moniker, reconstructed from its `path`/
/// `symbol` columns (`path#qualified.symbol`) rather than a stored column —
/// cheap, and always in sync with the nodes actually on disk.
fn full_moniker_map(storage: &SqliteStorage) -> Result<HashMap<String, NodeId>, StorageError> {
    Ok(storage
        .all_nodes()?
        .into_iter()
        .map(|n| (moniker::build(&n.path, &n.symbol), n.id))
        .collect())
}

fn upsert_all_edges(
    storage: &mut SqliteStorage,
    root: &Path,
    project_index: &ProjectIndex,
    files: &[PathBuf],
    moniker_to_id: &HashMap<String, NodeId>,
) -> Result<usize, StorageError> {
    // `upsert_edge`'s natural key is (source_id, target_id, kind) — a repeat
    // resolve (the same call site re-parsed, or CALLS_DYNAMIC fanning out to
    // a candidate already reached another way) upserts the same row rather
    // than adding one. Dedup here too, so the reported count matches what's
    // actually stored instead of counting write attempts.
    let mut distinct_edges = HashSet::new();
    parse_files_bounded(root, files, |_rel, parsed| {
        for edge in project_index.resolve(parsed) {
            if let (Some(&src_id), Some(&tgt_id)) = (
                moniker_to_id.get(&edge.source_moniker),
                moniker_to_id.get(&edge.target_moniker),
            ) {
                storage.upsert_edge(&Edge {
                    id: 0,
                    source_id: src_id,
                    target_id: tgt_id,
                    kind: edge.kind.clone(),
                    weight: 1.0,
                })?;
                distinct_edges.insert((src_id, tgt_id, edge.kind));
            }
        }
        Ok(())
    })?;
    Ok(distinct_edges.len())
}

/// AST-bounded chunk text per node (`impl.md` M3.7 Tier 2): the node's own
/// `line_start..=line_end` source span, reusing spans the parser already
/// computed rather than a second span-finder. Falls back to the bare
/// symbol name if the source file can't be read or the span is empty —
/// never fails the reindex over a missing chunk.
#[cfg(feature = "vector")]
fn build_vector_chunks(
    root: &Path,
    storage: &SqliteStorage,
) -> Result<Vec<(NodeId, String)>, StorageError> {
    let mut file_lines: HashMap<String, Vec<String>> = HashMap::new();
    let mut chunks = Vec::new();
    storage.for_each_node(&mut |node| {
        let lines = file_lines.entry(node.path.clone()).or_insert_with(|| {
            fs::read_to_string(root.join(&node.path))
                .map(|s| s.lines().map(str::to_string).collect())
                .unwrap_or_default()
        });
        let start = (node.line_start.saturating_sub(1)) as usize;
        let end = (node.line_end as usize).min(lines.len());
        let text = if start < end {
            lines[start..end].join("\n")
        } else {
            node.symbol.clone()
        };
        chunks.push((node.id, text));
    })?;
    Ok(chunks)
}

/// Full rebuild: parse everything, write into a fresh `.rebuild` file, then
/// atomically swap it in (Core Invariant 2). The safe default when there's
/// no existing index to diff against, or `should_bail_out` says the change
/// set is too large for a targeted update to be worth it.
pub(crate) fn full_reindex(
    root: &Path,
    weave_dir: &Path,
    active_db: &Path,
    files: &[PathBuf],
) -> Result<IndexStats, Box<dyn std::error::Error>> {
    let rebuild_db = weave_dir.join("graph.db.rebuild");
    if rebuild_db.exists() {
        fs::remove_file(&rebuild_db)?;
    }
    let mut storage = SqliteStorage::open(&rebuild_db)?;
    storage.begin_bulk_write()?;

    // Fused pass (was two passes): parse each file once, feeding both the
    // `ProjectIndex` (edge resolution's symbol table) and the node upserts.
    // The edges pass still needs its own parse — edge resolution needs the
    // *complete* index, which only exists after every file is seen — so the
    // floor is 2 parses/file, not 1. The `ParsedFile` is dropped per chunk;
    // nothing accumulates (PERF-06's streaming constraint, now bounded by
    // `PARSE_CHUNK` instead of one-at-a-time).
    let mut project_index = ProjectIndex::new();
    let mut total_symbols = 0usize;
    parse_files_bounded(root, files, |rel, parsed| {
        project_index.add_file(parsed);
        for symbol in &parsed.symbols {
            let node = Node {
                id: 0,
                repo_id: "local".to_string(),
                path: rel.to_string(),
                symbol: symbol.symbol.clone(),
                kind: symbol.kind.as_str().to_string(),
                line_start: symbol.line_start,
                line_end: symbol.line_end,
                signature: symbol.signature.clone(),
            };
            storage.upsert_node(&node)?;
            total_symbols += 1;
        }
        Ok(())
    })?;

    let moniker_to_id = full_moniker_map(&storage)?;
    let total_edges = upsert_all_edges(&mut storage, root, &project_index, files, &moniker_to_id)?;
    #[cfg(feature = "docs")]
    {
        let parsed_md = crate::docs::parse_markdown_files(files);
        crate::docs::upsert_doc_nodes(&mut storage, root, &parsed_md)?;
        crate::docs::upsert_doc_edges(&mut storage, root, &parsed_md)?;
        crate::docs::gc_orphaned_doc_topics(&storage)?;
    }
    #[cfg(feature = "notes")]
    {
        // Carry notes over from the old database the rebuild is replacing,
        // re-attaching by moniker (M2.10) — inside the same transaction.
        crate::notes::carry_over_and_prune(&storage, active_db, root, files, &moniker_to_id)?;
    }
    #[cfg(feature = "otel")]
    {
        // Carry imported trace spans over too (M3.3) — they match nodes
        // by symbol at query time, so a plain row copy suffices. A first
        // index has no previous database to copy from.
        if active_db.exists() {
            crate::traces::carry_over(active_db, &storage)?;
        }
    }
    #[cfg(feature = "fts")]
    // Full rebuild from the just-written `nodes` table (M3.7 Tier 1) —
    // `symbol_fts` is a derived index, never its own source of truth.
    storage.rebuild_fts_index()?;
    #[cfg(feature = "vector")]
    {
        let chunks = build_vector_chunks(root, &storage)?;
        storage.rebuild_vector_index(
            &weave_graph_core::embedding::MockEmbeddingProvider::new(),
            &chunks,
        )?;
    }
    storage.commit_bulk_write()?;
    storage.checkpoint_wal()?;
    drop(storage);
    fs::rename(&rebuild_db, active_db)?;

    Ok(IndexStats {
        files: files.len(),
        symbols: total_symbols,
        edges: total_edges,
    })
}

/// Targeted update: purge only `changed` files' nodes/edges — bidirectional,
/// Core Invariant 3 — then re-resolve *every* file's edges, so an edge from
/// an unchanged file into a changed one (dropped by the purge, since it
/// touches the changed file's node ids) gets correctly recreated. Still
/// stages into a copy of `active_db` under `.rebuild` and atomically swaps
/// it in, so a crash mid-update can never corrupt the active database.
pub(crate) fn incremental_reindex(
    root: &Path,
    weave_dir: &Path,
    active_db: &Path,
    files: &[PathBuf],
    changed: &[String],
) -> Result<IndexStats, Box<dyn std::error::Error>> {
    let project_index = build_project_index(root, files);

    let rebuild_db = weave_dir.join("graph.db.rebuild");
    if rebuild_db.exists() {
        fs::remove_file(&rebuild_db)?;
    }
    fs::copy(active_db, &rebuild_db)?;
    let mut storage = SqliteStorage::open(&rebuild_db)?;

    storage.begin_bulk_write()?;
    for rel in changed {
        storage.purge_file_edges("local", rel)?;
        storage.purge_file_nodes("local", rel)?;
    }

    let changed_files: Vec<PathBuf> = files
        .iter()
        .filter(|path| changed.iter().any(|c| c == &rel_path(root, path)))
        .cloned()
        .collect();
    upsert_all_nodes(&mut storage, root, &changed_files)?;

    let moniker_to_id = full_moniker_map(&storage)?;
    let total_edges = upsert_all_edges(&mut storage, root, &project_index, files, &moniker_to_id)?;
    #[cfg(feature = "docs")]
    {
        let all_parsed_md = crate::docs::parse_markdown_files(files);
        let changed_md: Vec<_> = all_parsed_md
            .iter()
            .filter(|(path, _)| changed.iter().any(|c| c == &rel_path(root, path)))
            .cloned()
            .collect();
        crate::docs::upsert_doc_nodes(&mut storage, root, &changed_md)?;
        crate::docs::upsert_doc_edges(&mut storage, root, &all_parsed_md)?;
        crate::docs::gc_orphaned_doc_topics(&storage)?;
    }
    #[cfg(feature = "notes")]
    {
        // Notes ride inside the copied database — the purge leaves their
        // target_node_id dangling (no FK enforcement is enabled on this
        // connection); reattach_and_prune re-resolves by moniker below,
        // explicitly nulling out anything that no longer resolves (M2.10).
        crate::notes::reattach_and_prune(&storage, root, files, &moniker_to_id)?;
    }
    #[cfg(feature = "fts")]
    storage.rebuild_fts_index()?;
    #[cfg(feature = "vector")]
    {
        let chunks = build_vector_chunks(root, &storage)?;
        storage.rebuild_vector_index(
            &weave_graph_core::embedding::MockEmbeddingProvider::new(),
            &chunks,
        )?;
    }
    storage.commit_bulk_write()?;
    let total_symbols = storage.all_nodes()?.len();
    drop(storage);
    fs::rename(&rebuild_db, active_db)?;

    Ok(IndexStats {
        files: files.len(),
        symbols: total_symbols,
        edges: total_edges,
    })
}

#[cfg(test)]
mod tests;
