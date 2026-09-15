use std::collections::HashMap;

use crate::model::NodeId;

/// Community id assigned by `louvain_communities` — a stable partition key,
/// not necessarily contiguous before renumbering (it is, after).
pub type CommunityId = u32;

/// Single-level Louvain modularity optimization — deriving architectural
/// modules from clustering — over an undirected, weighted graph. Real
/// Louvain adds a second phase that aggregates each
/// community into a super-node and re-runs phase one on the coarsened
/// graph; this stops once phase one converges, which already produces
/// genuine community structure for a file-dependency graph. Upgrade path:
/// aggregate by community and call this again on the result, if finer
/// clustering is ever needed.
pub fn louvain_communities(
    nodes: &[NodeId],
    edges: &[(NodeId, NodeId, f64)],
) -> HashMap<NodeId, CommunityId> {
    if nodes.is_empty() {
        return HashMap::new();
    }

    let adjacency = undirected_adjacency(edges);
    let total_weight: f64 = adjacency.values().flatten().map(|(_, w)| w).sum::<f64>() / 2.0;
    if total_weight <= 0.0 {
        return renumber(nodes, &nodes.iter().map(|&n| (n, n)).collect());
    }

    let degree: HashMap<NodeId, f64> = nodes
        .iter()
        .map(|&n| {
            let d = adjacency
                .get(&n)
                .map(|ns| ns.iter().map(|(_, w)| w).sum())
                .unwrap_or(0.0);
            (n, d)
        })
        .collect();

    let mut community: HashMap<NodeId, NodeId> = nodes.iter().map(|&n| (n, n)).collect();
    let mut community_degree: HashMap<NodeId, f64> = degree.clone();
    let two_m = 2.0 * total_weight;

    let mut improved = true;
    while improved {
        improved = false;
        for &node in nodes {
            if move_to_best_community(
                node,
                &adjacency,
                &degree,
                two_m,
                &mut community,
                &mut community_degree,
            ) {
                improved = true;
            }
        }
    }

    renumber(nodes, &community)
}

fn undirected_adjacency(edges: &[(NodeId, NodeId, f64)]) -> HashMap<NodeId, Vec<(NodeId, f64)>> {
    let mut merged: HashMap<(NodeId, NodeId), f64> = HashMap::new();
    for &(a, b, w) in edges {
        if a == b || w <= 0.0 {
            continue;
        }
        let key = if a <= b { (a, b) } else { (b, a) };
        *merged.entry(key).or_insert(0.0) += w;
    }

    let mut adjacency: HashMap<NodeId, Vec<(NodeId, f64)>> = HashMap::new();
    for ((a, b), w) in merged {
        adjacency.entry(a).or_default().push((b, w));
        adjacency.entry(b).or_default().push((a, w));
    }
    adjacency
}

/// Moves `node` into whichever neighboring community (or its own) gives the
/// highest modularity gain. Returns `true` if that changed its community.
fn move_to_best_community(
    node: NodeId,
    adjacency: &HashMap<NodeId, Vec<(NodeId, f64)>>,
    degree: &HashMap<NodeId, f64>,
    two_m: f64,
    community: &mut HashMap<NodeId, NodeId>,
    community_degree: &mut HashMap<NodeId, f64>,
) -> bool {
    let node_degree = degree.get(&node).copied().unwrap_or(0.0);
    let current = community.get(&node).copied().unwrap_or(node);

    let mut weight_to_community: HashMap<NodeId, f64> = HashMap::new();
    if let Some(neighbors) = adjacency.get(&node) {
        for &(neighbor, w) in neighbors {
            if neighbor == node {
                continue;
            }
            let c = community.get(&neighbor).copied().unwrap_or(neighbor);
            *weight_to_community.entry(c).or_insert(0.0) += w;
        }
    }

    *community_degree.entry(current).or_insert(0.0) -= node_degree;

    let gain_of = |candidate: NodeId, community_degree: &HashMap<NodeId, f64>| -> f64 {
        let k_in = weight_to_community.get(&candidate).copied().unwrap_or(0.0);
        let sigma_tot = community_degree.get(&candidate).copied().unwrap_or(0.0);
        k_in - sigma_tot * node_degree / two_m
    };

    let mut best = current;
    let mut best_gain = gain_of(current, community_degree);
    for &candidate in weight_to_community.keys() {
        if candidate == current {
            continue;
        }
        let gain = gain_of(candidate, community_degree);
        if gain > best_gain {
            best_gain = gain;
            best = candidate;
        }
    }

    *community_degree.entry(best).or_insert(0.0) += node_degree;
    if best != current {
        community.insert(node, best);
        true
    } else {
        false
    }
}

/// Renumbers raw community keys (founding node ids) into small, stable,
/// zero-based ids in `nodes`' own order — cosmetic, but keeps output
/// deterministic across runs on the same input.
fn renumber(nodes: &[NodeId], community: &HashMap<NodeId, NodeId>) -> HashMap<NodeId, CommunityId> {
    let mut seen: HashMap<NodeId, CommunityId> = HashMap::new();
    let mut next_id: CommunityId = 0;
    let mut result = HashMap::with_capacity(nodes.len());
    for &node in nodes {
        let raw = community.get(&node).copied().unwrap_or(node);
        let id = *seen.entry(raw).or_insert_with(|| {
            let id = next_id;
            next_id += 1;
            id
        });
        result.insert(node, id);
    }
    result
}

#[cfg(test)]
mod tests;
