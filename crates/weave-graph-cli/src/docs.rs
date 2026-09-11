//! `docs` feature (`plan.md` §2.1, `impl.md` M2.0): wires
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
const LINKS_TO: &str = "LINKS_TO";
const EXPLAINS_RATIONALE: &str = "EXPLAINS_RATIONALE";

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

type SymbolIndex = HashMap<String, Vec<NodeId>>;

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
/// call resolution.
fn doc_note_ids_by_title(storage: &SqliteStorage) -> Result<SymbolIndex, StorageError> {
    let mut map: SymbolIndex = HashMap::new();
    for n in storage
        .all_nodes()?
        .into_iter()
        .filter(|n| n.kind == DOC_NOTE_KIND)
    {
        map.entry(n.symbol).or_default().push(n.id);
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
    for n in storage
        .all_nodes()?
        .into_iter()
        .filter(|n| n.kind != DOC_NOTE_KIND && n.kind != DOC_TOPIC_KIND)
    {
        let short = n
            .symbol
            .rsplit("::")
            .next()
            .unwrap_or(&n.symbol)
            .to_string();
        by_full.entry(n.symbol.clone()).or_default().push(n.id);
        by_short.entry(short).or_default().push(n.id);
    }
    Ok((by_full, by_short))
}

fn normalize_code_ref(raw: &str) -> String {
    let trimmed = raw.trim();
    let without_call = trimmed.strip_suffix("()").unwrap_or(trimmed);
    without_call.replace('.', "::")
}

fn resolve_code_ref<'a>(
    raw: &str,
    by_full: &'a SymbolIndex,
    by_short: &'a SymbolIndex,
) -> Option<&'a [NodeId]> {
    let normalized = normalize_code_ref(raw);
    if let Some(ids) = by_full.get(&normalized) {
        return Some(ids);
    }
    let short = normalized.rsplit("::").next().unwrap_or(&normalized);
    by_short.get(short).map(|v| v.as_slice())
}

/// Resolves every parsed note's wikilinks/code-refs into `LINKS_TO`/
/// `EXPLAINS_RATIONALE` edges. Always runs over `all_parsed` (every
/// currently-known note), not just the changed subset — an unchanged
/// note's link into a just-purged-and-reinserted note needs re-resolving
/// against that note's new node id, the same reasoning `index.rs`'s
/// `upsert_all_edges` already applies to code. An unresolved wikilink or
/// code reference is dropped, never a dangling edge.
pub(crate) fn upsert_doc_edges(
    storage: &mut SqliteStorage,
    root: &Path,
    all_parsed: &[(PathBuf, ParsedMarkdown)],
) -> Result<usize, StorageError> {
    let note_id_by_path = doc_note_id_by_path(storage)?;
    let note_ids_by_title = doc_note_ids_by_title(storage)?;
    let (code_by_full, code_by_short) = code_symbol_ids(storage)?;

    let mut total = 0;
    for (path, doc) in all_parsed {
        let rel = rel_path(root, path);
        let Some(&source_id) = note_id_by_path.get(&rel) else {
            continue;
        };

        for link in &doc.links {
            let Some(target_ids) = note_ids_by_title.get(&link.target) else {
                continue;
            };
            for &target_id in target_ids {
                if target_id == source_id {
                    continue;
                }
                storage.upsert_edge(&Edge {
                    id: 0,
                    source_id,
                    target_id,
                    kind: LINKS_TO.to_string(),
                    weight: 1.0,
                })?;
                total += 1;
            }
        }

        for code_ref in &doc.code_refs {
            let Some(target_ids) = resolve_code_ref(code_ref, &code_by_full, &code_by_short) else {
                continue;
            };
            for &target_id in target_ids {
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
