use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::Mutex;

use rayon::prelude::*;

use weave_graph_core::{Edge, Node, NodeId, Storage, StorageError};
use weave_graph_parse::{ParsedFile, ProjectIndex, parse_file};
use weave_graph_store_sqlite::SqliteStorage;

pub(crate) struct IndexStats {
    pub(crate) files: usize,
    pub(crate) symbols: usize,
    pub(crate) edges: usize,
}

#[cfg(test)]
static FAIL_PROMOTION_FOR: Mutex<Option<PathBuf>> = Mutex::new(None);

#[cfg(test)]
fn fail_next_promotion(active_db: &Path) {
    if let Ok(mut failure) = FAIL_PROMOTION_FOR.lock() {
        *failure = Some(active_db.to_path_buf());
    }
}

fn promote_rebuild(rebuild_db: &Path, active_db: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    if let Ok(mut failure) = FAIL_PROMOTION_FOR.lock()
        && failure.as_ref().is_some_and(|path| path == active_db)
    {
        *failure = None;
        return Err(std::io::Error::other("injected rebuild promotion failure"));
    }
    fs::rename(rebuild_db, active_db)
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
pub(crate) fn build_project_index_from_storage(
    storage: &dyn weave_graph_core::Storage,
) -> Result<(ProjectIndex, HashMap<u32, NodeId>), weave_graph_core::StorageError> {
    let mut project_index = ProjectIndex::new();
    let mut moniker_to_node_id = HashMap::new();
    storage.for_each_node(&mut |node| {
        let moniker = weave_graph_parse::moniker::build(&node.path, &node.symbol);
        let short_name = node
            .symbol
            .rsplit("::")
            .next()
            .unwrap_or(&node.symbol)
            .to_string();
        let moniker_id = project_index.add_symbol(&moniker, &short_name);
        moniker_to_node_id.insert(moniker_id, node.id);
    })?;
    Ok((project_index, moniker_to_node_id))
}

/// Files parsed concurrently — the parallel-parse bound. Each in-flight
/// `ParsedFile` is KBs, so the worst-case working set stays orders of
/// magnitude under Invariant 4's 80 MB ceiling: a streaming property
/// kept by construction, not by hope.
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
    let mut failure = None;
    for chunk in files.chunks(PARSE_CHUNK) {
        let parsed: Vec<_> = chunk
            .par_iter()
            .map(|file_path| {
                let rel = rel_path(root, file_path);
                let parsed = match fs::read_to_string(file_path) {
                    Ok(source) => match parse_file(Path::new(&rel), &source) {
                        Some(Ok(parsed)) => Some(parsed),
                        Some(Err(err)) => {
                            eprintln!("skipping {}: {err}", file_path.display());
                            None
                        }
                        None => None,
                    },
                    Err(_) => None,
                };
                (rel, parsed)
            })
            .collect();
        for (rel, parsed) in parsed {
            if let Some(parsed) = parsed
                && failure.is_none()
                && let Err(err) = fold(&rel, &parsed)
            {
                failure = Some(err);
            }
        }
    }
    failure.map_or(Ok(()), Err)
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
) -> Result<Vec<(String, NodeId)>, StorageError> {
    let mut ids = Vec::new();
    parse_files_bounded(root, files, |rel, parsed| {
        let nodes = parsed
            .symbols
            .iter()
            .map(|symbol| Node {
                id: 0,
                repo_id: "local".to_string(),
                path: rel.to_string(),
                symbol: symbol.symbol.clone(),
                kind: symbol.kind.as_str().to_string(),
                line_start: symbol.line_start,
                line_end: symbol.line_end,
                signature: symbol.signature.clone(),
            })
            .collect::<Vec<_>>();
        ids.extend(
            storage
                .upsert_nodes(&nodes)?
                .into_iter()
                .map(|id| (rel.to_string(), id)),
        );
        Ok(())
    })?;
    Ok(ids)
}

fn upsert_all_edges(
    storage: &mut SqliteStorage,
    root: &Path,
    project_index: &ProjectIndex,
    files: &[PathBuf],
    moniker_id_to_node: &HashMap<u32, NodeId>,
) -> Result<(), StorageError> {
    parse_files_bounded(root, files, |rel, parsed| {
        let (edges, unresolved) = project_index.resolve_ids(parsed);
        let resolved = edges
            .into_iter()
            .filter_map(|edge| {
                let src_id = *moniker_id_to_node.get(&edge.source_id)?;
                let tgt_id = *moniker_id_to_node.get(&edge.target_id)?;
                Some(Edge {
                    id: 0,
                    source_id: src_id,
                    target_id: tgt_id,
                    kind: edge.kind,
                    weight: 1.0,
                })
            })
            .collect::<Vec<_>>();
        storage.upsert_edges(&resolved)?;
        use weave_graph_core::Storage;
        storage.purge_file_unresolved_refs("local", rel)?;
        storage.upsert_unresolved_refs("local", rel, &unresolved)?;
        Ok(())
    })?;
    Ok(())
}

/// AST-bounded chunk text per node: the node's own
/// `line_start..=line_end` source span, reusing spans the parser already
/// computed rather than a second span-finder. Falls back to the bare
/// symbol name if the source file can't be read or the span is empty —
/// never fails the reindex over a missing chunk.
#[cfg(feature = "vector")]
fn rebuild_vector_index(root: &Path, storage: &SqliteStorage) -> Result<(), StorageError> {
    let config_path = root.join(".weave").join("config.toml");
    let excluded_paths = crate::config::read_vector_exclude(&config_path);
    let embedder = weave_graph_core::embedding::MockEmbeddingProvider::new();
    let mut current_path = None;
    let mut source = String::new();
    let mut line_starts = Vec::new();
    storage.rebuild_vector_index_streaming(&embedder, |insert| {
        storage.for_each_node_by_path(&mut |node| {
            if excluded_paths
                .iter()
                .any(|prefix| path_is_excluded(&node.path, prefix))
            {
                return Ok(());
            }
            if current_path.as_deref() != Some(node.path.as_str()) {
                current_path = Some(node.path.clone());
                source = fs::read_to_string(root.join(&node.path)).unwrap_or_default();
                line_starts = source_line_starts(&source);
            }
            let text = source_span(&source, &line_starts, node.line_start, node.line_end)
                .unwrap_or(&node.symbol);
            insert(node.id, text)
        })
    })
}

#[cfg(feature = "vector")]
fn update_vector_index(
    root: &Path,
    storage: &SqliteStorage,
    paths: &[String],
) -> Result<(), StorageError> {
    let config_path = root.join(".weave").join("config.toml");
    let excluded_paths = crate::config::read_vector_exclude(&config_path);
    let embedder = weave_graph_core::embedding::MockEmbeddingProvider::new();
    let mut current_path = None;
    let mut source = String::new();
    let mut line_starts = Vec::new();
    storage.upsert_vector_index_streaming(&embedder, |insert| {
        storage.for_each_node_in_paths(paths, &mut |node| {
            if excluded_paths
                .iter()
                .any(|prefix| path_is_excluded(&node.path, prefix))
            {
                return Ok(());
            }
            if current_path.as_deref() != Some(node.path.as_str()) {
                current_path = Some(node.path.clone());
                source = fs::read_to_string(root.join(&node.path)).unwrap_or_default();
                line_starts = source_line_starts(&source);
            }
            let text = source_span(&source, &line_starts, node.line_start, node.line_end)
                .unwrap_or(&node.symbol);
            insert(node.id, text)
        })
    })
}

#[cfg(feature = "vector")]
fn source_line_starts(source: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(
            source
                .char_indices()
                .filter_map(|(index, ch)| (ch == '\n').then_some(index + ch.len_utf8())),
        )
        .collect()
}

#[cfg(feature = "vector")]
fn source_span<'a>(
    source: &'a str,
    line_starts: &[usize],
    line_start: u32,
    line_end: u32,
) -> Option<&'a str> {
    let start_line = line_start.saturating_sub(1) as usize;
    let end_line = line_end as usize;
    let start = *line_starts.get(start_line)?;
    let end = line_starts.get(end_line).copied().unwrap_or(source.len());
    (start < end).then_some(&source[start..end])
}

#[cfg(feature = "vector")]
fn path_is_excluded(path: &str, prefix: &str) -> bool {
    prefix.is_empty()
        || path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
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
        let nodes = parsed
            .symbols
            .iter()
            .map(|symbol| Node {
                id: 0,
                repo_id: "local".to_string(),
                path: rel.to_string(),
                symbol: symbol.symbol.clone(),
                kind: symbol.kind.as_str().to_string(),
                line_start: symbol.line_start,
                line_end: symbol.line_end,
                signature: symbol.signature.clone(),
            })
            .collect::<Vec<_>>();
        total_symbols += storage.upsert_nodes(&nodes)?.len();
        Ok(())
    })?;

    let (project_index, moniker_id_to_node) = build_project_index_from_storage(&storage)?;
    upsert_all_edges(
        &mut storage,
        root,
        &project_index,
        files,
        &moniker_id_to_node,
    )?;
    let total_edges = storage.edge_count()?;
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
        crate::notes::carry_over_and_prune(
            &storage,
            active_db,
            root,
            files,
            &project_index,
            &moniker_id_to_node,
        )?;
    }
    #[cfg(feature = "otel")]
    {
        // Carry imported trace spans over too — they match nodes
        // by symbol at query time, so a plain row copy suffices. A first
        // index has no previous database to copy from.
        if active_db.exists() {
            crate::traces::carry_over(active_db, &storage)?;
        }
    }
    #[cfg(feature = "vector")]
    {
        rebuild_vector_index(root, &storage)?;
    }
    storage.commit_bulk_write()?;
    storage.checkpoint_wal()?;
    drop(storage);
    promote_rebuild(&rebuild_db, active_db)?;

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
    let rebuild_db = weave_dir.join("graph.db.rebuild");
    if rebuild_db.exists() {
        fs::remove_file(&rebuild_db)?;
    }
    if changed.is_empty() {
        let storage = SqliteStorage::open(active_db)?;
        use weave_graph_core::Storage;
        return Ok(IndexStats {
            files: files.len(),
            symbols: storage.all_nodes()?.len(),
            edges: storage.edge_count()?,
        });
    }
    // Stage via SQLite's online backup, not a raw file copy: the backup
    // includes un-checkpointed WAL content a byte copy would miss, and it
    // streams page-by-page instead of one O(database) sequential write.
    {
        let source = SqliteStorage::open(active_db)?;
        source.backup_to(&rebuild_db)?;
    }
    let mut storage = SqliteStorage::open(&rebuild_db)?;

    use weave_graph_core::Storage;

    // --- 1. Compute Affected File Closure BEFORE Deletion ---
    let mut affected_files = changed.to_vec();
    let mut short_names = storage.short_symbol_names_for_paths("local", changed)?;
    affected_files.extend(storage.source_paths_for_target_paths("local", changed)?);

    // 1b. Collect short names of NEW symbols
    let changed_pathbufs: Vec<PathBuf> = files
        .iter()
        .filter(|path| changed.iter().any(|c| c == &rel_path(root, path)))
        .cloned()
        .collect();

    let _ = parse_files_bounded(root, &changed_pathbufs, |_rel, parsed| {
        for symbol in &parsed.symbols {
            short_names.insert(
                symbol
                    .symbol
                    .rsplit("::")
                    .next()
                    .unwrap_or(&symbol.symbol)
                    .to_string(),
            );
        }
        Ok(())
    });

    // 1c. Find files with unresolved refs to any of these short names
    for short_name in short_names {
        if let Ok(paths) = storage.get_files_with_unresolved_refs("local", &short_name) {
            affected_files.extend(paths);
        }
    }

    affected_files.sort();
    affected_files.dedup();

    let affected_pathbufs: Vec<PathBuf> = files
        .iter()
        .filter(|path| affected_files.iter().any(|a| a == &rel_path(root, path)))
        .cloned()
        .collect();

    // --- 2. Purge Changed Edges and Resolver Inputs ---
    storage.begin_bulk_write()?;
    for rel in changed {
        storage.purge_file_edges("local", rel)?;
        #[cfg(feature = "vector")]
        storage.purge_vector_paths(std::slice::from_ref(rel))?;
        storage.purge_file_unresolved_refs("local", rel)?;
    }

    // --- 3. Upsert New Nodes for Changed Files ---
    let changed_nodes = upsert_all_nodes(&mut storage, root, &changed_pathbufs)?;
    for rel in changed {
        let retained = changed_nodes
            .iter()
            .filter(|(path, _)| path == rel)
            .map(|(_, id)| *id)
            .collect::<Vec<_>>();
        storage.purge_file_nodes_except("local", rel, &retained)?;
    }
    #[cfg(feature = "vector")]
    update_vector_index(root, &storage, changed)?;

    // Core Invariant 3/4: We rebuild the project index from the database AFTER
    // upserting the changed nodes. This avoids parsing the entire repository just
    // to build the resolver.
    let (project_index, moniker_id_to_node) = build_project_index_from_storage(&storage)?;

    // --- 4. Re-resolve Edges ONLY for Affected Files ---
    // Changed-file purging already removed every stale endpoint. Purging an
    // unchanged affected file would also delete its unrelated inbound edges.
    upsert_all_edges(
        &mut storage,
        root,
        &project_index,
        &affected_pathbufs,
        &moniker_id_to_node,
    )?;
    let total_edges = storage.edge_count()?;
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
        // explicitly nulling out anything that no longer resolves.
        crate::notes::reattach_and_prune(
            &storage,
            root,
            files,
            &project_index,
            &moniker_id_to_node,
        )?;
    }
    storage.commit_bulk_write()?;
    let total_symbols = storage.all_nodes()?.len();
    drop(storage);
    promote_rebuild(&rebuild_db, active_db)?;

    Ok(IndexStats {
        files: files.len(),
        symbols: total_symbols,
        edges: total_edges,
    })
}

#[cfg(test)]
mod tests;
