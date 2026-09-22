//! Experimental deterministic vector retrieval for MCP.
//! The mock provider exercises storage and authorization boundaries but
//! is not presented as a production semantic model.

use weave_graph_core::embedding::MockEmbeddingProvider;
use weave_graph_core::synonym::expand_query;
use weave_graph_core::{MAX_SEARCH_LIMIT, Node, Storage, ranking::reciprocal_rank_fusion};

use crate::tools::SemanticSearchArgs;

const OVERSAMPLE: usize = 4;

/// A `weave_search_semantic` result plus an evidence-authority label:
/// `direct` is true when the query is a literal (case-insensitive)
/// substring of the node's own symbol name or path — never true merely
/// because the hit scored well on lexical/vector proximity. Purely
/// additive: never changes ranking, only labels it.
pub struct SemanticHit {
    pub node: Node,
    pub direct: bool,
}

fn is_direct_match(query: &str, node: &Node) -> bool {
    let query = query.to_ascii_lowercase();
    node.symbol.to_ascii_lowercase().contains(&query)
        || node.path.to_ascii_lowercase().contains(&query)
}

pub fn weave_search_semantic(
    storage: &dyn Storage,
    args: SemanticSearchArgs<'_>,
    visible: Option<&dyn Fn(&Node) -> bool>,
) -> Result<Vec<SemanticHit>, String> {
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
    let mut hits = Vec::new();
    for id in ids {
        let Some(node) = storage.get_node(id).map_err(|e| e.to_string())? else {
            continue;
        };
        if visible.is_none_or(|v| v(&node)) {
            let direct = is_direct_match(args.query, &node);
            hits.push(SemanticHit { node, direct });
        }
    }
    Ok(hits)
}

#[cfg(test)]
mod tests;
