//! `docs` feature: wires
//! `weave-graph-parse::markdown` into `weave index` — one `doc_note` node
//! per Markdown file, one `doc_topic` node per unique frontmatter
//! tag/alias, `LINKS_TO` edges for resolved wikilinks, and
//! `EXPLAINS_RATIONALE` edges for backtick code references that match an
//! already-indexed code symbol. Mirrors `index.rs`'s own purge-then-
//! reinsert-then-reresolve-everything shape so incremental correctness
//! (Core Invariant 3) holds for Markdown the same way it does for code.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use weave_graph_core::{Edge, Node, NodeId, Storage, StorageError};
use weave_graph_parse::markdown::{ParsedMarkdown, parse_markdown};
use weave_graph_store_sqlite::SqliteStorage;

pub(crate) const DOC_NOTE_KIND: &str = "doc_note";
pub(crate) const DOC_TOPIC_KIND: &str = "doc_topic";
pub(crate) const DOC_SECTION_KIND: &str = "doc_section";
const LINKS_TO: &str = "LINKS_TO";
const EXPLAINS_RATIONALE: &str = "EXPLAINS_RATIONALE";
const TAGGED: &str = "TAGGED";

pub(crate) fn is_markdown(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("md") | Some("markdown")
    )
}

fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

fn stem(rel: &str) -> String {
    Path::new(rel)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| rel.to_string())
}

/// Reads and parses every Markdown file in `files` — cheap enough (same
/// reasoning as `index::parse_all`) to always run over the full set, even
/// in incremental mode, since edge resolution below needs every note's
/// title known regardless of which ones actually changed.
pub(crate) fn parse_markdown_files(files: &[PathBuf]) -> Vec<(PathBuf, ParsedMarkdown)> {
    files
        .iter()
        .filter(|p| is_markdown(p))
        .filter_map(|p| {
            fs::read_to_string(p)
                .ok()
                .map(|src| (p.clone(), parse_markdown(&src)))
        })
        .collect()
}

/// One `doc_note` node per file in `parsed`, plus one `doc_topic` node per
/// unique tag/alias — `upsert_node`'s own natural key (`path=""` shared
/// across every topic node) dedups topics across files for free.
pub(crate) fn upsert_doc_nodes(
    storage: &mut SqliteStorage,
    root: &Path,
    parsed: &[(PathBuf, ParsedMarkdown)],
) -> Result<usize, StorageError> {
    let mut count = 0;
    for (path, doc) in parsed {
        let rel = rel_path(root, path);
        storage.upsert_node(&Node {
            id: 0,
            repo_id: "local".to_string(),
            path: rel.clone(),
            symbol: stem(&rel),
            kind: DOC_NOTE_KIND.to_string(),
            line_start: 0,
            line_end: 0,
            signature: String::new(),
        })?;
        count += 1;
        for tag in doc.tags.iter().chain(doc.aliases.iter()) {
            storage.upsert_node(&Node {
                id: 0,
                repo_id: "local".to_string(),
                path: String::new(),
                symbol: tag.clone(),
                kind: DOC_TOPIC_KIND.to_string(),
                line_start: 0,
                line_end: 0,
                signature: String::new(),
            })?;
            count += 1;
        }
    }
    Ok(count)
}

/// Code symbols keyed two ways: by their full stored `symbol` string, and
/// by its last `::`-separated segment — a backtick reference like
/// `` `AuthService.verify()` `` needs both, since the extractor's own
/// qualifier separator may not match the dotted form a human types. Each
/// entry carries the symbol's file path so same-directory references can
/// be disambiguated.
type SymbolIndex = HashMap<String, Vec<(NodeId, String)>>;

fn doc_note_id_by_path(storage: &SqliteStorage) -> Result<HashMap<String, NodeId>, StorageError> {
    Ok(storage
        .all_nodes()?
        .into_iter()
        .filter(|n| n.kind == DOC_NOTE_KIND)
        .map(|n| (n.path, n.id))
        .collect())
}

/// Fans out on an ambiguous title (two notes sharing a stem in different
/// folders) rather than picking one arbitrarily or dropping the link —
/// the same ambiguity idiom `ProjectIndex` already uses for by-short-name
/// call resolution. Carries each candidate's path so `#Section` anchors
/// can resolve against that specific note's headings.
fn doc_note_ids_by_title(
    storage: &SqliteStorage,
) -> Result<HashMap<String, Vec<(NodeId, String)>>, StorageError> {
    let mut map: HashMap<String, Vec<(NodeId, String)>> = HashMap::new();
    for n in storage
        .all_nodes()?
        .into_iter()
        .filter(|n| n.kind == DOC_NOTE_KIND)
    {
        map.entry(n.symbol).or_default().push((n.id, n.path));
    }
    Ok(map)
}

/// Code symbols keyed two ways: by their full stored `symbol` string, and
/// by its last `::`-separated segment — a backtick reference like
/// `` `AuthService.verify()` `` needs both, since the extractor's own
/// qualifier separator may not match the dotted form a human types.
fn code_symbol_ids(storage: &SqliteStorage) -> Result<(SymbolIndex, SymbolIndex), StorageError> {
    let mut by_full: SymbolIndex = HashMap::new();
    let mut by_short: SymbolIndex = HashMap::new();
    for n in storage.all_nodes()?.into_iter().filter(|n| {
        n.kind != DOC_NOTE_KIND && n.kind != DOC_TOPIC_KIND && n.kind != DOC_SECTION_KIND
    }) {
        let short = n
            .symbol
            .rsplit("::")
            .next()
            .unwrap_or(&n.symbol)
            .to_string();
        by_full
            .entry(n.symbol.clone())
            .or_default()
            .push((n.id, n.path.clone()));
        by_short.entry(short).or_default().push((n.id, n.path));
    }
    Ok((by_full, by_short))
}

fn normalize_code_ref(raw: &str) -> String {
    let trimmed = raw.trim();
    let without_call = trimmed.strip_suffix("()").unwrap_or(trimmed);
    without_call.replace('.', "::")
}

fn dir_of(path: &str) -> String {
    Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Narrow by directory when the short name alone is ambiguous: a
/// backtick reference in `notes/` that matches a same-named symbol in
/// both `src/notes/` and `src/other/` points at the same-directory one.
/// A single same-directory candidate is disambiguation; anything else
/// keeps the documented fan-out (never an arbitrary pick).
fn disambiguate(candidates: &[(NodeId, String)], note_dir: &str) -> Vec<NodeId> {
    let same_dir: Vec<NodeId> = candidates
        .iter()
        .filter(|(_, path)| dir_of(path) == note_dir)
        .map(|(id, _)| *id)
        .collect();
    if same_dir.len() == 1 {
        same_dir
    } else {
        candidates.iter().map(|(id, _)| *id).collect()
    }
}

fn resolve_code_ref(
    raw: &str,
    by_full: &SymbolIndex,
    by_short: &SymbolIndex,
    note_dir: &str,
) -> Option<Vec<NodeId>> {
    let normalized = normalize_code_ref(raw);
    let candidates = if let Some(ids) = by_full.get(&normalized) {
        ids
    } else {
        let short = normalized.rsplit("::").next().unwrap_or(&normalized);
        by_short.get(short)?
    };
    Some(disambiguate(candidates, note_dir))
}

/// Resolves every parsed note's wikilinks/code-refs into `LINKS_TO`/
/// `EXPLAINS_RATIONALE` edges, links each note to its own topic nodes
/// (`TAGGED`), and resolves `[[Note#Section]]` anchors against the
/// target note's headings (falling back to the note itself when the
/// heading doesn't exist — an anchor pointing nowhere is never
/// fabricated). Always runs over `all_parsed` (every currently-known
/// note), not just the changed subset — an unchanged note's link into a
/// just-purged-and-reinserted note needs re-resolving against that
/// note's new node id, the same reasoning `index.rs`'s `upsert_all_edges`
/// already applies to code. An unresolved wikilink or code reference is
/// dropped, never a dangling edge.
pub(crate) fn upsert_doc_edges(
    storage: &mut SqliteStorage,
    root: &Path,
    all_parsed: &[(PathBuf, ParsedMarkdown)],
) -> Result<usize, StorageError> {
    let note_id_by_path = doc_note_id_by_path(storage)?;
    let note_ids_by_title = doc_note_ids_by_title(storage)?;
    let (code_by_full, code_by_short) = code_symbol_ids(storage)?;
    let topic_ids = topic_id_by_symbol(storage)?;
    let headings_by_path: HashMap<String, Vec<String>> = all_parsed
        .iter()
        .map(|(path, doc)| (rel_path(root, path), doc.headings.clone()))
        .collect();

    let mut total = 0;
    for (path, doc) in all_parsed {
        let rel = rel_path(root, path);
        let Some(&source_id) = note_id_by_path.get(&rel) else {
            continue;
        };

        // Note -> its own topics: the inbound edges the orphan-topic GC
        // sweep (gc_orphaned_doc_topics) prunes against — a tag removed
        // from the frontmatter means the topic loses its last edge here.
        for tag in doc.tags.iter().chain(doc.aliases.iter()) {
            if let Some(&topic_id) = topic_ids.get(tag) {
                storage.upsert_edge(&Edge {
                    id: 0,
                    source_id,
                    target_id: topic_id,
                    kind: TAGGED.to_string(),
                    weight: 1.0,
                })?;
                total += 1;
            }
        }

        for link in &doc.links {
            let Some(target_ids) = note_ids_by_title.get(&link.target) else {
                continue;
            };
            for &(target_id, ref target_path) in target_ids {
                if target_id == source_id {
                    continue;
                }
                let target = match &link.section {
                    Some(section) => section_target(
                        storage,
                        target_id,
                        target_path,
                        section,
                        headings_by_path
                            .get(target_path.as_str())
                            .map(Vec::as_slice)
                            .unwrap_or(&[]),
                    )?,
                    None => target_id,
                };
                storage.upsert_edge(&Edge {
                    id: 0,
                    source_id,
                    target_id: target,
                    kind: LINKS_TO.to_string(),
                    weight: 1.0,
                })?;
                total += 1;
            }
        }

        let note_dir = dir_of(&rel).to_string();
        for code_ref in &doc.code_refs {
            let Some(target_ids) =
                resolve_code_ref(code_ref, &code_by_full, &code_by_short, &note_dir)
            else {
                continue;
            };
            for target_id in target_ids {
                storage.upsert_edge(&Edge {
                    id: 0,
                    source_id,
                    target_id,
                    kind: EXPLAINS_RATIONALE.to_string(),
                    weight: 1.0,
                })?;
                total += 1;
            }
        }
    }
    Ok(total)
}

fn topic_id_by_symbol(storage: &SqliteStorage) -> Result<HashMap<String, NodeId>, StorageError> {
    Ok(storage
        .all_nodes()?
        .into_iter()
        .filter(|n| n.kind == DOC_TOPIC_KIND)
        .map(|n| (n.symbol, n.id))
        .collect())
}

/// `[[Note#Section]]` target resolution: a heading match in the target
/// note becomes (or reuses) a `doc_section` child node under the note's
/// own path; no matching heading falls back to the parent note — the
/// anchor narrows when it can, never guesses.
fn section_target(
    storage: &mut SqliteStorage,
    note_id: NodeId,
    note_path: &str,
    section: &str,
    headings: &[String],
) -> Result<NodeId, StorageError> {
    let matched = headings.iter().any(|h| h.eq_ignore_ascii_case(section));
    if !matched {
        return Ok(note_id);
    }
    let stem = stem(note_path);
    storage.upsert_node(&Node {
        id: 0,
        repo_id: "local".to_string(),
        path: note_path.to_string(),
        symbol: format!("{stem}#{section}"),
        kind: DOC_SECTION_KIND.to_string(),
        line_start: 0,
        line_end: 0,
        signature: String::new(),
    })
}

/// Orphan-topic GC: a topic that lost
/// its last inbound `TAGGED` edge — e.g. a tag deleted from a note's
/// frontmatter — is swept rather than lingering forever. Inbound-only by
/// design: outbound edges to a topic aren't a reason to keep it.
pub(crate) fn gc_orphaned_doc_topics(storage: &SqliteStorage) -> Result<u64, StorageError> {
    storage.purge_orphaned_nodes_by_kind(DOC_TOPIC_KIND)
}

#[cfg(test)]
mod tests;
