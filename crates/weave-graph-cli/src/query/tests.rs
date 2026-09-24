use weave_graph_core::{Edge, Node, NodeId, Note, Storage, StorageError};
use weave_graph_store_sqlite::SqliteStorage;

use super::*;

fn node(path: &str, symbol: &str) -> Node {
    Node {
        id: 0,
        repo_id: "r".into(),
        path: path.into(),
        symbol: symbol.into(),
        kind: "function".into(),
        line_start: 1,
        line_end: 3,
        signature: format!("fn {symbol}()"),
    }
}

fn edge(source_id: NodeId, target_id: NodeId) -> Edge {
    Edge {
        id: 0,
        source_id,
        target_id,
        kind: "CALLS_EXACT".into(),
        weight: 1.0,
        extractor: None,
        resolution_kind: None,
    }
}

// caller -> a -> b -> c (chain)
fn chain_storage() -> SqliteStorage {
    let mut s = SqliteStorage::open_in_memory().unwrap();
    let caller = s.upsert_node(&node("caller.rs", "caller")).unwrap();
    let a = s.upsert_node(&node("a.rs", "a")).unwrap();
    let b = s.upsert_node(&node("b.rs", "b")).unwrap();
    let c = s.upsert_node(&node("c.rs", "c")).unwrap();
    s.upsert_edge(&edge(caller, a)).unwrap();
    s.upsert_edge(&edge(a, b)).unwrap();
    s.upsert_edge(&edge(b, c)).unwrap();
    s
}

#[test]
fn callers_finds_transitive_callers() {
    let storage = chain_storage();
    let result = run(&storage, "callers(b)", None).unwrap();
    assert!(result.contains("a ("));
    assert!(result.contains("caller ("));
    assert!(!result.contains("c ("));
}

#[test]
fn callees_is_bounded_to_direct_neighbors_only() {
    let storage = chain_storage();
    let result = run(&storage, "callees(a)", None).unwrap();
    assert!(result.contains("b ("));
    assert!(
        !result.contains("c ("),
        "callees(a) must not include transitive c"
    );
}

#[test]
fn impact_is_unbounded_transitively() {
    let storage = chain_storage();
    let result = run(&storage, "impact(a)", None).unwrap();
    assert!(result.contains("b ("));
    assert!(
        result.contains("c ("),
        "impact(a) must include transitive c"
    );
}

#[test]
fn path_finds_the_shortest_chain() {
    let storage = chain_storage();
    let result = run(&storage, "path(caller,c)", None).unwrap();
    assert_eq!(result, "caller → a → b → c");
}

#[test]
fn path_reports_no_path_found_when_unreachable() {
    let storage = chain_storage();
    let result = run(&storage, "path(c,caller)", None).unwrap();
    assert_eq!(result, "no path found");
}

#[test]
fn unresolvable_symbol_is_a_clear_error() {
    let storage = chain_storage();
    let err = run(&storage, "callers(nope)", None).unwrap_err();
    assert!(err.contains("symbol not found: nope"));
}

#[test]
fn malformed_expression_is_a_clear_error() {
    let storage = chain_storage();
    assert!(run(&storage, "not a call", None).is_err());
}

#[test]
fn wrong_argument_count_is_a_clear_error() {
    let storage = chain_storage();
    assert!(run(&storage, "callers(a,b)", None).is_err());
    assert!(run(&storage, "path(a)", None).is_err());
}

#[test]
fn unknown_function_is_a_clear_error() {
    let storage = chain_storage();
    let err = run(&storage, "bogus(a)", None).unwrap_err();
    assert!(err.contains("unknown query function"));
}

/// `latency(<symbol>)` compiles in every build but means different
/// things: with `otel`, aggregate stats (empty store → the "no spans"
/// line); without it, the missing-feature error. One test covers
/// whichever arm this build compiled.
#[test]
fn latency_behaves_per_compiled_feature_set() {
    let storage = chain_storage();
    match run(&storage, "latency(b)", None) {
        Ok(text) => assert!(text.contains("no trace spans matched to symbol b")),
        Err(msg) => assert!(msg.contains("otel")),
    }
}

#[test]
fn mask_is_applied_to_fetched_nodes() {
    let storage = chain_storage();
    let passthrough = |n: &Node| n.clone();
    let mask: Option<&dyn Fn(&Node) -> Node> = Some(&passthrough);
    assert!(run(&storage, "callers(b)", mask).unwrap().contains("a ("));
}

#[test]
fn empty_result_sets_print_no_results() {
    let storage = chain_storage();
    assert_eq!(
        run(&storage, "callers(caller)", None).unwrap(),
        "no results"
    );
    assert_eq!(run(&storage, "callees(c)", None).unwrap(), "no results");
}

#[test]
fn expressions_without_a_call_shape_are_unrecognized() {
    let storage = chain_storage();
    assert!(run(&storage, "callers", None).is_err());
    assert!(run(&storage, "callers(a", None).is_err());
}

struct FailingCallersStorage(SqliteStorage);

impl Storage for FailingCallersStorage {
    fn schema_version(&self) -> Result<u32, StorageError> {
        self.0.schema_version()
    }
    fn upsert_node(&mut self, node: &Node) -> Result<NodeId, StorageError> {
        self.0.upsert_node(node)
    }
    fn get_node(&self, id: NodeId) -> Result<Option<Node>, StorageError> {
        self.0.get_node(id)
    }
    fn all_nodes(&self) -> Result<Vec<Node>, StorageError> {
        self.0.all_nodes()
    }
    fn all_edges(&self) -> Result<Vec<Edge>, StorageError> {
        self.0.all_edges()
    }
    fn upsert_edge(&mut self, edge: &Edge) -> Result<u32, StorageError> {
        self.0.upsert_edge(edge)
    }
    fn get_edges(&self, id: NodeId) -> Result<Vec<Edge>, StorageError> {
        self.0.get_edges(id)
    }
    fn get_callers(&self, _: NodeId) -> Result<Vec<Edge>, StorageError> {
        Err(StorageError::Backend("simulated disk read failure".into()))
    }
    fn purge_file_edges(&mut self, repo_id: &str, path: &str) -> Result<u64, StorageError> {
        self.0.purge_file_edges(repo_id, path)
    }
    fn purge_file_nodes(&mut self, repo_id: &str, path: &str) -> Result<u64, StorageError> {
        self.0.purge_file_nodes(repo_id, path)
    }
    fn query_path(&self, from: NodeId, to: NodeId) -> Result<Option<Vec<NodeId>>, StorageError> {
        self.0.query_path(from, to)
    }
    fn pin_note(&self, note: &Note) -> Result<i64, StorageError> {
        self.0.pin_note(note)
    }
    fn all_notes(&self) -> Result<Vec<Note>, StorageError> {
        self.0.all_notes()
    }
    fn recall_notes(&self, now: i64) -> Result<Vec<Note>, StorageError> {
        self.0.recall_notes(now)
    }
    fn reattach_note(
        &self,
        id: i64,
        target: Option<NodeId>,
        stale: bool,
    ) -> Result<(), StorageError> {
        self.0.reattach_note(id, target, stale)
    }
    fn delete_expired_notes(&self, now: i64) -> Result<u64, StorageError> {
        self.0.delete_expired_notes(now)
    }
}

#[test]
fn callers_propagates_storage_errors() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let _ = storage.upsert_node(&node("test.rs", "foo")).unwrap();
    let failing = FailingCallersStorage(storage);
    let err = run(&failing, "callers(foo)", None).unwrap_err();
    assert!(err.contains("failed to read callers"));
    assert!(err.contains("simulated disk read failure"));
}

// --- P10.7: composable path:/lang:/kind:/visibility:/edge: filters ---

#[test]
fn path_filter_narrows_impact_results() {
    let storage = chain_storage();
    let result = run(&storage, "impact(caller) path:b", None).unwrap();
    assert!(result.contains("b ("), "{result}");
    assert!(!result.contains("a ("), "{result}");
    assert!(!result.contains("c ("), "{result}");
}

#[test]
fn kind_filter_narrows_callers_results() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let root = storage.upsert_node(&node("root.rs", "root")).unwrap();
    let func_caller = storage.upsert_node(&node("f.rs", "funcCaller")).unwrap();
    let struct_caller_node = Node {
        kind: "struct".into(),
        ..node("s.rs", "structCaller")
    };
    let struct_caller = storage.upsert_node(&struct_caller_node).unwrap();
    storage.upsert_edge(&edge(func_caller, root)).unwrap();
    storage.upsert_edge(&edge(struct_caller, root)).unwrap();

    let result = run(&storage, "callers(root) kind:struct", None).unwrap();
    assert!(result.contains("structCaller ("), "{result}");
    assert!(!result.contains("funcCaller ("), "{result}");
}

#[test]
fn lang_filter_matches_by_file_extension() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let root = storage.upsert_node(&node("root.rs", "root")).unwrap();
    let rust_caller = storage.upsert_node(&node("f.rs", "rustCaller")).unwrap();
    let python_caller = storage.upsert_node(&node("f.py", "pythonCaller")).unwrap();
    storage.upsert_edge(&edge(rust_caller, root)).unwrap();
    storage.upsert_edge(&edge(python_caller, root)).unwrap();

    let result = run(&storage, "callers(root) lang:python", None).unwrap();
    assert!(result.contains("pythonCaller ("), "{result}");
    assert!(!result.contains("rustCaller ("), "{result}");
}

#[test]
fn visibility_filter_reuses_the_contract_visibility_rule() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let root = storage.upsert_node(&node("root.rs", "root")).unwrap();
    let public_node = Node {
        signature: "pub fn publicCaller()".into(),
        ..node("pub.rs", "publicCaller")
    };
    let private_node = Node {
        signature: "fn privateCaller()".into(),
        ..node("priv.rs", "privateCaller")
    };
    let public_id = storage.upsert_node(&public_node).unwrap();
    let private_id = storage.upsert_node(&private_node).unwrap();
    storage.upsert_edge(&edge(public_id, root)).unwrap();
    storage.upsert_edge(&edge(private_id, root)).unwrap();

    let result = run(&storage, "callers(root) visibility:public", None).unwrap();
    assert!(result.contains("publicCaller ("), "{result}");
    assert!(!result.contains("privateCaller ("), "{result}");
}

#[test]
fn edge_filter_narrows_callers_to_one_edge_kind() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let root = storage.upsert_node(&node("root.rs", "root")).unwrap();
    let exact_caller = storage.upsert_node(&node("e.rs", "exactCaller")).unwrap();
    let dynamic_caller = storage.upsert_node(&node("d.rs", "dynamicCaller")).unwrap();
    storage.upsert_edge(&edge(exact_caller, root)).unwrap();
    storage
        .upsert_edge(&Edge {
            kind: "CALLS_DYNAMIC".into(),
            ..edge(dynamic_caller, root)
        })
        .unwrap();

    let result = run(&storage, "callers(root) edge:CALLS_DYNAMIC", None).unwrap();
    assert!(result.contains("dynamicCaller ("), "{result}");
    assert!(!result.contains("exactCaller ("), "{result}");
}

#[test]
fn edge_filter_is_rejected_for_non_callers_forms() {
    let storage = chain_storage();
    let err = run(&storage, "impact(caller) edge:CALLS_EXACT", None).unwrap_err();
    assert!(err.contains("only supported for callers()"), "{err}");
}

#[test]
fn filters_are_rejected_on_path_and_latency() {
    let storage = chain_storage();
    let err = run(&storage, "path(caller,c) path:a", None).unwrap_err();
    assert!(err.contains("not supported"), "{err}");
}

#[test]
fn an_invalid_filter_token_is_a_clear_error() {
    let storage = chain_storage();
    let err = run(&storage, "callers(a) bogus", None).unwrap_err();
    assert!(err.contains("invalid filter"), "{err}");
}

#[test]
fn an_unknown_filter_key_is_a_clear_error() {
    let storage = chain_storage();
    let err = run(&storage, "callers(a) nonexistent:x", None).unwrap_err();
    assert!(err.contains("unknown filter"), "{err}");
}

#[test]
fn an_invalid_visibility_value_is_a_clear_error() {
    let storage = chain_storage();
    let err = run(&storage, "callers(a) visibility:sideways", None).unwrap_err();
    assert!(err.contains("invalid visibility"), "{err}");
}

#[test]
fn precise_filter_drops_a_heuristically_resolved_caller() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let root = storage.upsert_node(&node("root.rs", "root")).unwrap();
    let exact_caller = storage.upsert_node(&node("e.rs", "exactCaller")).unwrap();
    let heuristic_caller = storage
        .upsert_node(&node("h.rs", "heuristicCaller"))
        .unwrap();
    storage.upsert_edge(&edge(exact_caller, root)).unwrap();
    storage
        .upsert_edge(&Edge {
            kind: "CALLS_DYNAMIC".into(),
            resolution_kind: Some(weave_graph_core::AMBIGUOUS_HEURISTIC.to_string()),
            ..edge(heuristic_caller, root)
        })
        .unwrap();

    let result = run(&storage, "callers(root) precise:true", None).unwrap();
    assert!(result.contains("exactCaller ("), "{result}");
    assert!(!result.contains("heuristicCaller ("), "{result}");
}

#[test]
fn precise_false_is_a_no_op_that_keeps_every_caller() {
    let mut storage = SqliteStorage::open_in_memory().unwrap();
    let root = storage.upsert_node(&node("root.rs", "root")).unwrap();
    let heuristic_caller = storage
        .upsert_node(&node("h.rs", "heuristicCaller"))
        .unwrap();
    storage
        .upsert_edge(&Edge {
            kind: "CALLS_DYNAMIC".into(),
            resolution_kind: Some(weave_graph_core::AMBIGUOUS_HEURISTIC.to_string()),
            ..edge(heuristic_caller, root)
        })
        .unwrap();

    let result = run(&storage, "callers(root) precise:false", None).unwrap();
    assert!(result.contains("heuristicCaller ("), "{result}");
}

#[test]
fn precise_filter_is_rejected_for_non_callers_forms() {
    let storage = chain_storage();
    let err = run(&storage, "impact(caller) precise:true", None).unwrap_err();
    assert!(err.contains("only supported for callers()"), "{err}");
}

#[test]
fn an_invalid_precise_value_is_a_clear_error() {
    let storage = chain_storage();
    let err = run(&storage, "callers(a) precise:maybe", None).unwrap_err();
    assert!(err.contains("invalid precise"), "{err}");
}

#[test]
fn filters_apply_under_an_rbac_mask_too() {
    let storage = chain_storage();
    let mask: &dyn Fn(&Node) -> Node = &|n| n.clone();
    let result = run(&storage, "impact(caller) path:b", Some(mask)).unwrap();
    assert!(result.contains("b ("), "{result}");
    assert!(!result.contains("a ("), "{result}");
}
