use super::*;

#[test]
fn composite_key_joins_the_three_parts_with_double_colons() {
    assert_eq!(
        composite_key("repo-a", "src/utils.ts", "helper"),
        "repo-a::src/utils.ts::helper"
    );
}

#[test]
fn empty_graph_has_no_components() {
    assert!(tarjan_scc(&[], &[]).is_empty());
}

#[test]
fn nodes_with_no_edges_are_each_their_own_singleton_component() {
    let sccs = tarjan_scc(&[1, 2, 3], &[]);
    assert_eq!(sccs.len(), 3);
    assert!(sccs.iter().all(|c| c.len() == 1));
}

#[test]
fn a_linear_chain_has_no_cycle_and_stays_all_singletons() {
    // 1 -> 2 -> 3, no edge back to 1 — a real cross-repo dependency chain
    // with no cycle should never be reported as one SCC.
    let sccs = tarjan_scc(&[1, 2, 3], &[(1, 2), (2, 3)]);
    assert_eq!(sccs.len(), 3);
    assert!(sccs.iter().all(|c| c.len() == 1));
}

#[test]
fn a_deliberate_three_repo_cycle_is_reported_as_one_scc() {
    // A -> B -> C -> A: exactly the circular cross-repo dependency
    // that is Tarjan's SCC's reason to exist here.
    let sccs = tarjan_scc(&[1, 2, 3], &[(1, 2), (2, 3), (3, 1)]);
    assert_eq!(sccs.len(), 1);
    let mut only = sccs[0].clone();
    only.sort_unstable();
    assert_eq!(only, vec![1, 2, 3]);
}

#[test]
fn a_cycle_alongside_an_unrelated_acyclic_node_isolates_correctly() {
    // A <-> B cycle, plus an independent C with no edges at all — the
    // cycle must not swallow the unrelated node, and vice versa.
    let sccs = tarjan_scc(&[1, 2, 3], &[(1, 2), (2, 1)]);
    assert_eq!(
        sccs.len(),
        2,
        "expected the {{1,2}} cycle plus singleton {{3}}"
    );
    let sizes: Vec<usize> = {
        let mut s: Vec<usize> = sccs.iter().map(|c| c.len()).collect();
        s.sort_unstable();
        s
    };
    assert_eq!(sizes, vec![1, 2]);
}

#[test]
fn a_self_loop_is_its_own_single_node_component() {
    let sccs = tarjan_scc(&[1], &[(1, 1)]);
    assert_eq!(sccs.len(), 1);
    assert_eq!(sccs[0], vec![1]);
}
