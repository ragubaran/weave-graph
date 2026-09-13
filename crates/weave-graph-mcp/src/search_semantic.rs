//! `weave_search_semantic` (CORE-03/SEC-04, feature `vector`): the same
//! reference `MockEmbeddingProvider` and SEC-01 pre-truncation visibility
//! filter as `weave-graph-cli::search::run_semantic`, wired for MCP
//! callers instead of the CLI.

use weave_graph_core::embedding::MockEmbeddingProvider;
use weave_graph_core::{MAX_SEARCH_LIMIT, Node, Storage};

use crate::tools::SemanticSearchArgs;

const OVERSAMPLE: usize = 4;

pub fn weave_search_semantic(
    storage: &dyn Storage,
    args: SemanticSearchArgs<'_>,
    visible: Option<&dyn Fn(&Node) -> bool>,
) -> Result<Vec<Node>, String> {
    let embedder = MockEmbeddingProvider::new();
    let limit = args.limit.min(MAX_SEARCH_LIMIT);
    let ids = storage
        .search_vector(&embedder, args.query, limit, OVERSAMPLE, visible)
        .map_err(|e| e.to_string())?;
    let mut nodes = Vec::new();
    for id in ids {
        let Some(node) = storage.get_node(id).map_err(|e| e.to_string())? else {
            continue;
        };
        if visible.is_none_or(|v| v(&node)) {
            nodes.push(node);
        }
    }
    Ok(nodes)
}

#[cfg(test)]
mod tests;
