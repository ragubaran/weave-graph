use super::*;

#[test]
fn empty_graph_returns_no_communities() {
    assert!(louvain_communities(&[], &[]).is_empty());
}

#[test]
fn nodes_with_no_edges_each_get_their_own_community() {
    let nodes = [1, 2, 3];
    let communities = louvain_communities(&nodes, &[]);
    let ids: std::collections::HashSet<CommunityId> = communities.values().copied().collect();
    assert_eq!(ids.len(), 3, "no edges means no reason to merge anything");
}

#[test]
fn two_dense_clusters_joined_by_one_weak_edge_are_kept_separate() {
    // Cluster A: 1-2-3 fully connected. Cluster B: 4-5-6 fully connected.
    // One weak bridge edge 3-4 should not be enough to merge them.
    let nodes = [1, 2, 3, 4, 5, 6];
    let edges = [
        (1, 2, 5.0),
        (1, 3, 5.0),
        (2, 3, 5.0),
        (4, 5, 5.0),
        (4, 6, 5.0),
        (5, 6, 5.0),
        (3, 4, 1.0),
    ];
    let communities = louvain_communities(&nodes, &edges);

    assert_eq!(communities[&1], communities[&2]);
    assert_eq!(communities[&2], communities[&3]);
    assert_eq!(communities[&4], communities[&5]);
    assert_eq!(communities[&5], communities[&6]);
    assert_ne!(
        communities[&1], communities[&4],
        "the two dense triangles must not merge across one weak bridge edge"
    );
}

#[test]
fn a_single_fully_connected_cluster_stays_one_community() {
    let nodes = [1, 2, 3, 4];
    let edges = [
        (1, 2, 1.0),
        (1, 3, 1.0),
        (1, 4, 1.0),
        (2, 3, 1.0),
        (2, 4, 1.0),
        (3, 4, 1.0),
    ];
    let communities = louvain_communities(&nodes, &edges);
    let ids: std::collections::HashSet<CommunityId> = communities.values().copied().collect();
    assert_eq!(ids.len(), 1);
}

#[test]
fn self_loops_and_zero_or_negative_weights_are_ignored() {
    let nodes = [1, 2];
    let edges = [(1, 1, 5.0), (1, 2, 0.0), (1, 2, -1.0)];
    let communities = louvain_communities(&nodes, &edges);
    // No real edges survive filtering, so no merge pressure exists.
    assert_ne!(communities[&1], communities[&2]);
}

#[test]
fn community_ids_are_stable_and_zero_based_after_renumbering() {
    let nodes = [10, 20, 30];
    let edges = [(10, 20, 1.0)];
    let communities = louvain_communities(&nodes, &edges);
    let mut ids: Vec<CommunityId> = communities.values().copied().collect();
    ids.sort_unstable();
    ids.dedup();
    assert!(ids.iter().all(|&id| id < nodes.len() as CommunityId));
}

#[test]
fn parallel_edges_between_the_same_pair_accumulate_weight() {
    let nodes = [1, 2, 3];
    // 1-2 has heavy accumulated weight from repeated symbol-level edges;
    // 2-3 is a single light edge. 1 and 2 should end up together.
    let edges = [(1, 2, 3.0), (1, 2, 3.0), (1, 2, 3.0), (2, 3, 0.1)];
    let communities = louvain_communities(&nodes, &edges);
    assert_eq!(communities[&1], communities[&2]);
}
