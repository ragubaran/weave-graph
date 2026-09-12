//! `weave search "<query>"` (`impl.md` M3.7 Tier 1, feature `fts`):
//! deterministic BM25 symbol search with synonym expansion, zero neural
//! weights. Results are filtered (not masked-in-place) through M3.0's
//! `RbacGuard` when `--as` is given — same precedent as `weave report`.

use std::path::Path;

use weave_graph_core::synonym::expand_query;
use weave_graph_core::{Node, Storage, StorageError};
use weave_graph_store_sqlite::SqliteStorage;

use crate::open_storage_for_read;

/// Ranked, RBAC-filtered hits for `query` — split out from `cmd_search` so
/// tests assert on real data, not just "didn't panic while printing."
pub(crate) fn run(
    storage: &SqliteStorage,
    query: &str,
    limit: usize,
    visible: Option<&dyn Fn(&Node) -> bool>,
) -> Result<Vec<Node>, StorageError> {
    let expanded = expand_query(query);
    let mut nodes = Vec::new();
    for id in storage.search_symbols(&expanded, limit)? {
        let Some(node) = storage.get_node(id)? else {
            continue;
        };
        if visible.is_none_or(|v| v(&node)) {
            nodes.push(node);
        }
    }
    Ok(nodes)
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
    // in (Feature Isolation, `AGENTS.md` §1.8).
    #[cfg(feature = "rbac")]
    let guard = as_subject.map(|s| crate::rbac::guard_for(root, Some(s)));
    #[cfg(feature = "rbac")]
    let checker = guard.as_ref().map(|g| |n: &Node| g.visible(n));
    #[cfg(feature = "rbac")]
    let visible: Option<&dyn Fn(&Node) -> bool> =
        checker.as_ref().map(|c| c as &dyn Fn(&Node) -> bool);
    #[cfg(not(feature = "rbac"))]
    let (visible, _) = (None::<&dyn Fn(&Node) -> bool>, as_subject);

    let nodes = run(&storage, query, limit, visible)?;
    if nodes.is_empty() {
        println!("No visible matches for \"{query}\".");
        return Ok(());
    }
    for node in &nodes {
        println!("{} ({}:{})", node.symbol, node.path, node.line_start);
    }
    Ok(())
}

/// Tier 2 (`impl.md` M3.7): binary-ANN-then-int8-rerank semantic search
/// over AST-bounded chunks, via the `MockEmbeddingProvider` reference
/// implementation — a real deployment supplies its own `EmbeddingProvider`
/// (same "boundary here, real provider elsewhere" shape as `AuthProvider`).
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
    let mut nodes = Vec::new();
    for id in storage.search_vector(&embedder, query, limit, OVERSAMPLE)? {
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
