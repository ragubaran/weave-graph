use super::*;

/// A mock that exercises the trait's default method bodies: trace-span
/// support is opt-in per backend — the defaults refuse writes and
/// report an empty read rather than forcing minimal `Storage`
/// implementations (test mocks, benches) to stub rows.
#[derive(Default)]
struct MinimalStorage;

impl Storage for MinimalStorage {
    fn get_node(&self, _: NodeId) -> Result<Option<Node>, StorageError> {
        Ok(None)
    }
    fn get_edges(&self, _: NodeId) -> Result<Vec<Edge>, StorageError> {
        Ok(Vec::new())
    }
    fn get_callers(&self, _: NodeId) -> Result<Vec<Edge>, StorageError> {
        Ok(Vec::new())
    }
    fn upsert_node(&mut self, _: &Node) -> Result<NodeId, StorageError> {
        Ok(0)
    }
    fn upsert_edge(&mut self, _: &Edge) -> Result<u32, StorageError> {
        Ok(0)
    }
    fn query_path(&self, _: NodeId, _: NodeId) -> Result<Option<Vec<NodeId>>, StorageError> {
        Ok(None)
    }
    fn schema_version(&self) -> Result<u32, StorageError> {
        Ok(0)
    }
    fn all_nodes(&self) -> Result<Vec<Node>, StorageError> {
        Ok(Vec::new())
    }
    fn all_edges(&self) -> Result<Vec<Edge>, StorageError> {
        Ok(Vec::new())
    }
    fn purge_file_edges(&mut self, _: &str, _: &str) -> Result<u64, StorageError> {
        Ok(0)
    }
    fn purge_file_nodes(&mut self, _: &str, _: &str) -> Result<u64, StorageError> {
        Ok(0)
    }
    fn pin_note(&self, _: &Note) -> Result<i64, StorageError> {
        Ok(0)
    }
    fn all_notes(&self) -> Result<Vec<Note>, StorageError> {
        Ok(Vec::new())
    }
    fn recall_notes(&self, _: i64) -> Result<Vec<Note>, StorageError> {
        Ok(Vec::new())
    }
    fn reattach_note(&self, _: i64, _: Option<NodeId>, _: bool) -> Result<(), StorageError> {
        Ok(())
    }
    fn delete_expired_notes(&self, _: i64) -> Result<u64, StorageError> {
        Ok(0)
    }
}

#[test]
fn trace_span_defaults_refuse_writes_and_read_empty() {
    let storage = MinimalStorage;
    assert!(storage.all_trace_spans().unwrap().is_empty());
    let err = storage
        .upsert_trace_span(&crate::trace::TraceSpan {
            trace_id: "t".into(),
            span_id: "s".into(),
            parent_span_id: None,
            service: None,
            name: "n".into(),
            symbol: None,
            path: None,
            start_us: 0,
            duration_us: 0,
            status_code: "Unset".into(),
        })
        .unwrap_err();
    assert!(err.to_string().contains("does not support trace spans"));
}

#[test]
fn search_symbols_default_refuses_with_an_unsupported_error() {
    let storage = MinimalStorage;
    let err = storage.search_symbols("anything", 10, None).unwrap_err();
    assert!(err.to_string().contains("does not support symbol search"));
}

#[test]
fn find_all_symbols_default_refuses_with_an_unsupported_error() {
    let storage = MinimalStorage;
    let err = storage.find_all_symbols("anything").unwrap_err();
    assert!(
        err.to_string()
            .contains("does not support exhaustive symbol search")
    );
}

/// The optional-surface defaults every minimal backend inherits: edge
/// counting without materialization, streaming iteration, and no-op
/// resolver-input persistence. Exercising them here pins the defaults a
/// backend may rely on without overriding.
#[test]
fn trait_defaults_cover_the_optional_surface() {
    let mut storage = MinimalStorage;

    assert_eq!(storage.edge_count().unwrap(), 0);

    let mut seen_nodes = Vec::new();
    storage
        .for_each_node(&mut |node| seen_nodes.push(node.id))
        .unwrap();
    assert!(seen_nodes.is_empty());

    let mut seen_edges = Vec::new();
    storage
        .for_each_edge(&mut |edge| seen_edges.push(edge.id))
        .unwrap();
    assert!(seen_edges.is_empty());

    storage
        .upsert_unresolved_refs("r", "a.rs", &["sym".to_string()])
        .unwrap();
    assert_eq!(storage.purge_file_unresolved_refs("r", "a.rs").unwrap(), 0);
    assert!(
        storage
            .get_files_with_unresolved_refs("r", "sym")
            .unwrap()
            .is_empty()
    );
    assert!(
        storage
            .get_unresolved_refs_for_path("r", "a.rs")
            .unwrap()
            .is_empty()
    );

    // Default streams via `for_each_node` rather than materializing
    // `all_nodes()` — same empty-graph contract, provable here; the
    // "finds a real match" case is covered on the real backend
    // (`weave-graph-store-sqlite`'s own `get_node_by_symbol` tests),
    // since `MinimalStorage`'s `all_nodes()` is always empty.
    assert!(storage.get_node_by_symbol("anything").unwrap().is_none());
}

#[cfg(feature = "vector")]
#[test]
fn search_vector_default_refuses_with_an_unsupported_error() {
    let storage = MinimalStorage;
    let embedder = crate::embedding::MockEmbeddingProvider::new();
    let err = storage
        .search_vector(&embedder, "anything", 10, 4, None)
        .unwrap_err();
    assert!(err.to_string().contains("does not support vector search"));
}

#[cfg(feature = "vector")]
#[test]
fn find_similar_node_pairs_default_refuses_with_an_unsupported_error() {
    let storage = MinimalStorage;
    let err = storage
        .find_similar_node_pairs(&[1, 2], 0.85, 8)
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("does not support semantic-coupling search")
    );
}
