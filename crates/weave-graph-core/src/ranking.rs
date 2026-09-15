//! Deterministic rank-combination helpers shared by retrieval surfaces.

use std::collections::{HashMap, HashSet};

use crate::NodeId;

const RRF_K: f64 = 60.0;

/// Combines ranked node-id lists with reciprocal-rank fusion.
/// Ties resolve by node id so output is reproducible across runs.
pub fn reciprocal_rank_fusion(lists: &[&[NodeId]], limit: usize) -> Vec<NodeId> {
    let mut scores = HashMap::<NodeId, f64>::new();
    for list in lists {
        let mut seen = HashSet::new();
        for (rank, id) in list.iter().copied().enumerate() {
            if seen.insert(id) {
                *scores.entry(id).or_default() += 1.0 / (RRF_K + rank as f64 + 1.0);
            }
        }
    }
    let mut ranked: Vec<(NodeId, f64)> = scores.into_iter().collect();
    ranked.sort_unstable_by(|(left_id, left_score), (right_id, right_score)| {
        right_score
            .total_cmp(left_score)
            .then_with(|| left_id.cmp(right_id))
    });
    ranked.truncate(limit);
    ranked.into_iter().map(|(id, _)| id).collect()
}

#[cfg(test)]
mod tests {
    use super::reciprocal_rank_fusion;

    #[test]
    fn shared_hits_outrank_single_list_hits() {
        assert_eq!(
            reciprocal_rank_fusion(&[&[3, 1, 2], &[2, 3, 4]], 4),
            vec![3, 2, 1, 4]
        );
    }

    #[test]
    fn ties_and_duplicates_are_deterministic() {
        assert_eq!(reciprocal_rank_fusion(&[&[2, 2], &[1]], 3), vec![1, 2]);
    }
}
