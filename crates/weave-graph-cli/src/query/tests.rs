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
