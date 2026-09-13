use super::*;

// Compile-time check: the trait must stay dyn-compatible so callers can
// hold `Box<dyn Storage>` without committing to a backend at compile
// time.
#[allow(dead_code)]
fn assert_object_safe(_: &dyn Storage) {}

/// A mock that exercises the trait's default method bodies: trace-span
/// support is opt-in per backend (M3.3) — the defaults refuse writes and
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
    let err = storage.search_symbols("anything", 10).unwrap_err();
    assert!(err.to_string().contains("does not support symbol search"));
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
