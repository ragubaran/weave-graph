//! Cross-agent memory graph (`impl.md` M2.10): CLI verbs (`weave note
//! pin`/`list`) and the reindex hooks — moniker-based reattachment,
//! blake3 staleness recompute, and opportunistic expiry deletion, all
//! inside the reindex's own bulk-write transaction.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use weave_graph_core::notes::{EPHEMERAL_TTL_SECS, hash_span};
use weave_graph_core::{NodeId, Note, NoteTier, Storage, StorageError};
use weave_graph_parse::moniker;
use weave_graph_store_sqlite::SqliteStorage;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub(crate) fn active_db(root: &Path) -> std::path::PathBuf {
    let weave_home_env = std::env::var("WEAVE_HOME").ok();
    crate::storage_location::resolve_data_dir(root, weave_home_env.as_deref())
        .path
        .join("graph.db")
}

/// Resolves `symbol` by exact match against the stored nodes (lowest id
/// wins on duplicates), mirroring the MCP tools' own resolution rule.
pub(crate) fn resolve_symbol(
    storage: &dyn Storage,
    symbol: &str,
) -> Option<weave_graph_core::Node> {
    storage
        .all_nodes()
        .ok()?
        .into_iter()
        .find(|n| n.symbol == symbol)
}

/// `weave note pin [--keep] <symbol> <text>`: hash the symbol's exact
/// current source span for the staleness signal, then persist.
pub(crate) fn cmd_note_pin(
    root: &Path,
    symbol: &str,
    text: &str,
    keep: bool,
    kind: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let active_db = active_db(root);
    let _lock = crate::lock::acquire(active_db.parent().unwrap_or(Path::new(".")))?;
    let storage = SqliteStorage::open(&active_db)?;

    let Some(node) = resolve_symbol(&storage, symbol) else {
        return Err(format!("symbol not found: {symbol}").into());
    };

    // Read the file once; the hash covers the symbol's exact span so a
    // same-line-range rewrite is detected later (never an mtime guess).
    let content_hash = fs::read_to_string(root.join(&node.path))
        .ok()
        .map(|source| hash_span(&source, node.line_start, node.line_end));

    let now = now_secs();
    let tier = if keep {
        NoteTier::Crystallized
    } else {
        NoteTier::Ephemeral
    };
    let note = Note {
        id: 0,
        target_node_id: Some(node.id),
        moniker: moniker::build(&node.path, &node.symbol),
        kind: kind.to_string(),
        tier,
        author: "human".to_string(),
        content: text.to_string(),
        content_hash,
        stale: false,
        expires_at: (tier == NoteTier::Ephemeral).then(|| now + EPHEMERAL_TTL_SECS),
        created_at: now,
    };
    let id = storage.pin_note(&note)?;
    println!("Pinned note #{id} to {symbol} ({})", note.tier.as_str());
    Ok(())
}

/// `weave note list` — the recall view: TTL-filtered, orphaned and stale
/// notes reported explicitly, never silently dropped.
pub(crate) fn cmd_note_list(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let storage = SqliteStorage::open(&active_db(root))?;
    let notes = storage.recall_notes(now_secs())?;
    if notes.is_empty() {
        println!("No notes.");
        return Ok(());
    }
    for note in &notes {
        let mut tags = vec![note.tier.as_str().to_string()];
        if note.stale {
            tags.push("stale".to_string());
        }
        if note.target_node_id.is_none() {
            tags.push("orphaned".to_string());
        }
        println!(
            "#{} [{}] {}: {}",
            note.id,
            tags.join(", "),
            note.moniker,
            note.content
        );
    }
    Ok(())
}

/// Moniker → the symbol's current `(line_start, line_end)`, derived from
/// the files `parse_all` already parsed — no second parse pass.
fn moniker_span_map(root: &Path, files: &[std::path::PathBuf]) -> HashMap<String, (u32, u32)> {
    let mut spans = HashMap::new();
    for path in files {
        if let Ok(source) = std::fs::read_to_string(path) {
            let rel = crate::index::rel_path(root, path);
            if let Some(Ok(parsed)) = weave_graph_parse::parse_file(Path::new(&rel), &source) {
                for symbol in &parsed.symbols {
                    spans.insert(
                        weave_graph_parse::moniker::build(&rel, &symbol.symbol),
                        (symbol.line_start, symbol.line_end),
                    );
                }
            }
        }
    }
    spans
}

fn file_source(root: &Path, file: &str, cache: &mut HashMap<String, String>) -> Option<String> {
    if let Some(hit) = cache.get(file) {
        return Some(hit.clone());
    }
    let source = fs::read_to_string(root.join(file)).ok()?;
    cache.insert(file.to_string(), source.clone());
    Some(source)
}

/// The reindex hook, called inside the already-open bulk-write
/// transaction of BOTH reindex paths (impl.md M2.10: "additive only", a
/// `#[cfg(feature = "notes")]` block after the existing upsert phases).
///
/// For every stored note: a live moniker re-attaches to the symbol's new
/// node id (purge-and-reinsert gives new ids, Core Invariant 3); a
/// crystallized note's staleness is recomputed by hashing the symbol's
/// current span (one file read per noted file, only when notes exist); a
/// moniker that no longer resolves orphans the note — reported by recall,
/// never dropped, never misattached. Finally, expired ephemerals are
/// opportunistically deleted — piggybacked, not a separate write pass.
pub(crate) fn reattach_and_prune(
    storage: &SqliteStorage,
    root: &Path,
    files: &[std::path::PathBuf],
    moniker_to_id: &HashMap<String, NodeId>,
) -> Result<(), StorageError> {
    let now = now_secs();
    let notes = storage.all_notes()?;
    if !notes.is_empty() {
        let spans = moniker_span_map(root, files);
        let mut sources: HashMap<String, String> = HashMap::new();

        for note in &notes {
            let target = moniker_to_id.get(&note.moniker).copied();
            let stale = match (note.tier, note.content_hash.as_ref(), target) {
                (NoteTier::Crystallized, Some(old_hash), Some(_)) => match (
                    spans.get(&note.moniker),
                    note.moniker.split_once('#').map(|(file, _)| file),
                ) {
                    (Some(&(line_start, line_end)), Some(file)) => {
                        file_source(root, file, &mut sources)
                            .map(|source| hash_span(&source, line_start, line_end) != *old_hash)
                    }
                    _ => None,
                }
                .unwrap_or(note.stale),
                _ => note.stale,
            };
            storage.reattach_note(note.id, target, stale)?;
        }
    }
    storage.delete_expired_notes(now)?;
    Ok(())
}

/// Full-reindex variant: the notes live in the OLD database, which the
/// rebuild swap is about to replace — copy them into the fresh rebuild
/// database with moniker-resolved (or orphaned) targets, then prune.
pub(crate) fn carry_over_and_prune(
    new_storage: &SqliteStorage,
    old_db: &Path,
    root: &Path,
    files: &[std::path::PathBuf],
    project_index: &weave_graph_parse::ProjectIndex,
    moniker_id_to_node: &HashMap<u32, NodeId>,
) -> Result<(), StorageError> {
    let old_storage = SqliteStorage::open(old_db)?;
    let notes = old_storage.all_notes()?;
    let spans = moniker_span_map(root, files);
    let mut sources: HashMap<String, String> = HashMap::new();

    for note in &notes {
        let target = project_index
            .get_moniker_id(&note.moniker)
            .and_then(|id| moniker_id_to_node.get(&id).copied());
        let stale = match (note.tier, note.content_hash.as_ref(), target) {
            (NoteTier::Crystallized, Some(old_hash), Some(_)) => match (
                spans.get(&note.moniker),
                note.moniker.split_once('#').map(|(file, _)| file),
            ) {
                (Some(&(line_start, line_end)), Some(file)) => {
                    file_source(root, file, &mut sources)
                        .map(|source| hash_span(&source, line_start, line_end) != *old_hash)
                }
                _ => None,
            }
            .unwrap_or(note.stale),
            _ => note.stale,
        };
        new_storage.pin_note(&Note {
            id: 0,
            target_node_id: target,
            moniker: note.moniker.clone(),
            kind: note.kind.clone(),
            tier: note.tier,
            author: note.author.clone(),
            content: note.content.clone(),
            content_hash: note.content_hash.clone(),
            stale,
            expires_at: note.expires_at,
            created_at: note.created_at,
        })?;
    }
    new_storage.delete_expired_notes(now_secs())?;
    Ok(())
}

#[cfg(test)]
mod tests;
