use super::*;

#[test]
fn node_and_edge_are_plain_equatable_data() {
    let a = Node {
        id: 1,
        repo_id: "r".into(),
        path: "src/lib.rs".into(),
        symbol: "foo".into(),
        kind: "function".into(),
        line_start: 1,
        line_end: 3,
        signature: "fn foo()".into(),
    };
    let b = a.clone();
    assert_eq!(a, b);

    let e = Edge {
        id: 1,
        source_id: 1,
        target_id: 2,
        kind: "CALLS_EXACT".into(),
        weight: 1.0,
    };
    assert_eq!(e.source_id, 1);
    assert_eq!(e.target_id, 2);
}
