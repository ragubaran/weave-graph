//! `weave search "<query>"` (feature `fts`): deterministic BM25 symbol
//! search with synonym expansion, zero neural weights. Results are
//! filtered (not masked-in-place) through the
//! `RbacGuard` when `--as` is given — same precedent as `weave report`.

use std::path::Path;

#[cfg(feature = "vector")]
use weave_graph_core::ranking::reciprocal_rank_fusion;
use weave_graph_core::synonym::expand_query;
use weave_graph_core::{MAX_SEARCH_LIMIT, Node, Storage, StorageError};
use weave_graph_store_sqlite::SqliteStorage;

use crate::open_storage_for_read;

fn bounded_limit(limit: usize) -> usize {
    limit.min(MAX_SEARCH_LIMIT)
}

/// Ranked, RBAC-filtered hits for `query` — split out from `cmd_search` so
/// tests assert on real data, not just "didn't panic while printing."
pub(crate) fn run(
    storage: &SqliteStorage,
    query: &str,
    limit: usize,
    visible: Option<&dyn Fn(&Node) -> bool>,
) -> Result<Vec<Node>, StorageError> {
    let expanded = expand_query(query);
    storage.search_symbol_nodes(&expanded, bounded_limit(limit), visible)
}

/// Same as [`run`], plus a bounded, case-insensitive literal-substring
/// fallback scan over every node's `symbol`/`signature`/`path` when FTS
/// finds nothing — a fallback, never a parallel signal: FTS and a
/// substring scan aren't independent signals the way lexical/vector are,
/// so this never runs when FTS already answered. Catches the tokenizer
/// gap FTS5 has by construction: `split_identifier` splits an identifier
/// into words at index time, so a query crossing a word-split boundary
/// (e.g. "kenExpira" against `JwtTokenExpirationHandler`) can never
/// `MATCH`, no matter how the query is expanded. Returns whether the
/// fallback path produced these results, so the CLI can label them.
pub(crate) fn run_with_fallback(
    storage: &SqliteStorage,
    query: &str,
    limit: usize,
    visible: Option<&dyn Fn(&Node) -> bool>,
) -> Result<(Vec<Node>, bool), StorageError> {
    let hits = run(storage, query, limit, visible)?;
    if !hits.is_empty() {
        return Ok((hits, false));
    }
    let query_lower = query.to_ascii_lowercase();
    let mut fallback: Vec<Node> = storage
        .all_nodes()?
        .into_iter()
        .filter(|n| visible.is_none_or(|v| v(n)))
        .filter(|n| {
            n.symbol.to_ascii_lowercase().contains(&query_lower)
                || n.signature.to_ascii_lowercase().contains(&query_lower)
                || n.path.to_ascii_lowercase().contains(&query_lower)
        })
        .collect();
    fallback.truncate(bounded_limit(limit));
    Ok((fallback, true))
}

pub(crate) fn cmd_search(
    root: &Path,
    query: &str,
    limit: usize,
    as_subject: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (storage, _db_path) = open_storage_for_read(root)?;
    // Same rule as `cmd_query`/`cmd_report`: masking only engages with an
    // explicit `--as <subject>`, never merely because `rbac` is compiled
    // in — enabling a feature must not change default behavior.
    #[cfg(feature = "rbac")]
    let guard = as_subject.map(|s| crate::rbac::guard_for(root, Some(s)));
    #[cfg(feature = "rbac")]
    let checker = guard.as_ref().map(|g| |n: &Node| g.visible(n));
    #[cfg(feature = "rbac")]
    let visible: Option<&dyn Fn(&Node) -> bool> =
        checker.as_ref().map(|c| c as &dyn Fn(&Node) -> bool);
    #[cfg(not(feature = "rbac"))]
    let (visible, _) = (None::<&dyn Fn(&Node) -> bool>, as_subject);

    let (nodes, is_fallback) = run_with_fallback(&storage, query, limit, visible)?;
    if nodes.is_empty() {
        println!("No visible matches for \"{query}\".");
        return Ok(());
    }
    if is_fallback {
        println!("No FTS match — showing literal substring hits:");
    }
    for node in &nodes {
        println!("{} ({}:{})", node.symbol, node.path, node.line_start);
    }
    Ok(())
}

/// Experimental vector path using deterministic mock embeddings.
/// The explicit label prevents benchmark scaffolding from being mistaken
/// for a production semantic model.
#[cfg(feature = "vector")]
const OVERSAMPLE: usize = 4;

#[cfg(feature = "vector")]
pub(crate) fn run_semantic(
    storage: &SqliteStorage,
    query: &str,
    limit: usize,
    visible: Option<&dyn Fn(&Node) -> bool>,
) -> Result<Vec<Node>, StorageError> {
    let embedder = weave_graph_core::embedding::MockEmbeddingProvider::new();
    let candidate_limit = bounded_limit(limit.saturating_mul(OVERSAMPLE));
    let vector_ids =
        storage.search_vector(&embedder, query, candidate_limit, OVERSAMPLE, visible)?;
    let lexical_ids: Vec<_> = run(storage, query, candidate_limit, visible)?
        .into_iter()
        .map(|node| node.id)
        .collect();
    let fused_ids = reciprocal_rank_fusion(&[&vector_ids, &lexical_ids], bounded_limit(limit));
    let mut nodes = Vec::with_capacity(fused_ids.len());
    for id in fused_ids {
        let Some(node) = storage.get_node(id)? else {
            continue;
        };
        if visible.is_none_or(|v| v(&node)) {
            nodes.push(node);
        }
    }
    Ok(nodes)
}

#[cfg(feature = "vector")]
pub(crate) fn cmd_search_semantic(
    root: &Path,
    query: &str,
    limit: usize,
    as_subject: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (storage, _db_path) = open_storage_for_read(root)?;
    #[cfg(feature = "rbac")]
    let guard = as_subject.map(|s| crate::rbac::guard_for(root, Some(s)));
    #[cfg(feature = "rbac")]
    let checker = guard.as_ref().map(|g| |n: &Node| g.visible(n));
    #[cfg(feature = "rbac")]
    let visible: Option<&dyn Fn(&Node) -> bool> =
        checker.as_ref().map(|c| c as &dyn Fn(&Node) -> bool);
    #[cfg(not(feature = "rbac"))]
    let (visible, _) = (None::<&dyn Fn(&Node) -> bool>, as_subject);

    let nodes = run_semantic(&storage, query, limit, visible)?;
    if nodes.is_empty() {
        println!("No visible semantic matches for \"{query}\".");
        return Ok(());
    }
    for node in &nodes {
        println!(
            "{} ({}:{}) [semantic]",
            node.symbol, node.path, node.line_start
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
