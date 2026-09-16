//! Experimental deterministic vector retrieval for MCP.
//! The mock provider exercises storage and authorization boundaries but
//! is not presented as a production semantic model.

use weave_graph_core::embedding::MockEmbeddingProvider;
use weave_graph_core::synonym::expand_query;
use weave_graph_core::{MAX_SEARCH_LIMIT, Node, Storage, ranking::reciprocal_rank_fusion};

use crate::tools::SemanticSearchArgs;

const OVERSAMPLE: usize = 4;

pub fn weave_search_semantic(
    storage: &dyn Storage,
    args: SemanticSearchArgs<'_>,
    visible: Option<&dyn Fn(&Node) -> bool>,
) -> Result<Vec<Node>, String> {
    let embedder = MockEmbeddingProvider::new();
    let limit = args.limit.min(MAX_SEARCH_LIMIT);
    let candidate_limit = limit.saturating_mul(OVERSAMPLE).max(limit);
    let vector_ids = storage
        .search_vector(&embedder, args.query, candidate_limit, OVERSAMPLE, visible)
        .map_err(|e| e.to_string())?;
    let expanded_query = expand_query(args.query);
    let lexical_ids = storage
        .search_symbols(&expanded_query, candidate_limit, visible)
        .map_err(|e| e.to_string())?;
    let ids = reciprocal_rank_fusion(&[&vector_ids, &lexical_ids], limit);
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
