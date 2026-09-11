use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

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
pub(crate) fn parse_all(
    root: &Path,
    files: &[PathBuf],
) -> (ProjectIndex, Vec<(PathBuf, ParsedFile)>) {
    let mut project_index = ProjectIndex::new();
    let mut parsed_files = Vec::with_capacity(files.len());
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
    parsed_files: &[(PathBuf, ParsedFile)],
) -> Result<usize, StorageError> {
    let mut total_symbols = 0;
    for (path, parsed) in parsed_files {
        let rel = rel_path(root, path);
        for symbol in &parsed.symbols {
            let node = Node {
                id: 0,
                repo_id: "local".to_string(),
                path: rel.clone(),
                symbol: symbol.symbol.clone(),
                kind: symbol.kind.as_str().to_string(),
                line_start: symbol.line_start,
                line_end: symbol.line_end,
                signature: symbol.signature.clone(),
            };
            storage.upsert_node(&node)?;
            total_symbols += 1;
        }
    }
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
    project_index: &ProjectIndex,
    parsed_files: &[(PathBuf, ParsedFile)],
    moniker_to_id: &HashMap<String, NodeId>,
) -> Result<usize, StorageError> {
    let mut total_edges = 0;
    for (_path, parsed) in parsed_files {
        for edge in project_index.resolve(parsed) {
            if let (Some(&src_id), Some(&tgt_id)) = (
                moniker_to_id.get(&edge.source_moniker),
                moniker_to_id.get(&edge.target_moniker),
            ) {
                storage.upsert_edge(&Edge {
                    id: 0,
                    source_id: src_id,
                    target_id: tgt_id,
                    kind: edge.kind,
                    weight: 1.0,
                })?;
                total_edges += 1;
            }
        }
    }
    Ok(total_edges)
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
    let (project_index, parsed_files) = parse_all(root, files);

    let rebuild_db = weave_dir.join("graph.db.rebuild");
    if rebuild_db.exists() {
        fs::remove_file(&rebuild_db)?;
    }
    let mut storage = SqliteStorage::open(&rebuild_db)?;
    storage.begin_bulk_write()?;
    let total_symbols = upsert_all_nodes(&mut storage, root, &parsed_files)?;
    let moniker_to_id = full_moniker_map(&storage)?;
    let total_edges =
        upsert_all_edges(&mut storage, &project_index, &parsed_files, &moniker_to_id)?;
    #[cfg(feature = "docs")]
    {
        let parsed_md = crate::docs::parse_markdown_files(files);
        crate::docs::upsert_doc_nodes(&mut storage, root, &parsed_md)?;
        crate::docs::upsert_doc_edges(&mut storage, root, &parsed_md)?;
    }
    #[cfg(feature = "notes")]
    {
        // Carry notes over from the old database the rebuild is replacing,
        // re-attaching by moniker (M2.10) — inside the same transaction.
        crate::notes::carry_over_and_prune(
            &storage,
            active_db,
            root,
            &parsed_files,
            &moniker_to_id,
        )?;
    }
    storage.commit_bulk_write()?;
    drop(storage);
    fs::rename(&rebuild_db, active_db)?;

    Ok(IndexStats {
        files: parsed_files.len(),
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
    let (project_index, parsed_files) = parse_all(root, files);

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

    let changed_parsed: Vec<(PathBuf, ParsedFile)> = parsed_files
        .iter()
        .filter(|(path, _)| changed.iter().any(|c| c == &rel_path(root, path)))
        .cloned()
        .collect();
    upsert_all_nodes(&mut storage, root, &changed_parsed)?;

    let moniker_to_id = full_moniker_map(&storage)?;
    let total_edges =
        upsert_all_edges(&mut storage, &project_index, &parsed_files, &moniker_to_id)?;
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
    }
    #[cfg(feature = "notes")]
    {
        // Notes ride inside the copied database — the purge leaves their
        // target_node_id dangling (no FK enforcement is enabled on this
        // connection); reattach_and_prune re-resolves by moniker below,
        // explicitly nulling out anything that no longer resolves (M2.10).
        crate::notes::reattach_and_prune(&storage, root, &parsed_files, &moniker_to_id)?;
    }
    storage.commit_bulk_write()?;
    let total_symbols = storage.all_nodes()?.len();
    drop(storage);
    fs::rename(&rebuild_db, active_db)?;

    Ok(IndexStats {
        files: parsed_files.len(),
        symbols: total_symbols,
        edges: total_edges,
    })
}

#[cfg(test)]
mod tests;
