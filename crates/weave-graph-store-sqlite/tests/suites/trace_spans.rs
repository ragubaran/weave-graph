//! Parameterized M3.3 trace-span suite (see `suites/` convention: every
//! test fn takes an `OpenFn` so the same bodies run against any backend).

use weave_graph_core::trace::TraceSpan;
use weave_graph_core::{Node, Storage};

pub type OpenFn = fn(&std::path::Path) -> Box<dyn Storage>;

fn span(trace_id: &str, span_id: &str, symbol: &str, duration_us: i64) -> TraceSpan {
    TraceSpan {
        trace_id: trace_id.to_string(),
        span_id: span_id.to_string(),
        parent_span_id: None,
        service: Some("api".to_string()),
        name: symbol.to_string(),
        symbol: Some(symbol.to_string()),
        path: Some("src/lib.rs".to_string()),
        start_us: 1_000_000,
        duration_us,
        status_code: "Ok".to_string(),
    }
}

fn indexed_node(storage: &mut dyn Storage) -> Node {
    let node = Node {
        id: 0,
        repo_id: "repo".into(),
        path: "src/lib.rs".into(),
        symbol: "handle".into(),
        kind: "function".into(),
        line_start: 1,
        line_end: 4,
        signature: "fn handle()".into(),
    };
    let id = storage.upsert_node(&node).unwrap();
    Node { id, ..node }
}

/// Close/reopen round trip, plus the upsert-conflict path: re-importing
/// the same `(trace_id, span_id)` replaces the row rather than
/// duplicating it.
pub fn trace_spans_survive_close_and_reopen(open: OpenFn) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("test.db");
    {
        let storage = open(&db);
        storage
            .upsert_trace_span(&span("t1", "s1", "handle", 2_000))
            .unwrap();
    }
    {
        let storage = open(&db);
        storage
            .upsert_trace_span(&span("t1", "s1", "handle", 9_999))
            .unwrap();
        storage
            .upsert_trace_span(&span("t1", "s2", "handle", 500))
            .unwrap();
        let spans = storage.all_trace_spans().unwrap();
        assert_eq!(spans.len(), 2);
        let replaced = spans.iter().find(|s| s.span_id == "s1").unwrap();
        assert_eq!(replaced.duration_us, 9_999);
        assert_eq!(replaced.symbol.as_deref(), Some("handle"));
        assert_eq!(spans[0].span_id, "s1");
        assert_eq!(spans[1].span_id, "s2");
    }
}

/// A reindex purge (nodes and both edge directions) leaves imported
/// spans untouched: they match nodes by symbol at query time, never by a
/// stored node id that a reindex could renumber.
pub fn trace_spans_survive_file_purge(open: OpenFn) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("test.db");
    let mut storage = open(&db);
    let node = indexed_node(storage.as_mut());
    storage
        .upsert_trace_span(&span("t1", "s1", &node.symbol, 2_000))
        .unwrap();
    storage.purge_file_edges("repo", "src/lib.rs").unwrap();
    storage.purge_file_nodes("repo", "src/lib.rs").unwrap();
    let spans = storage.all_trace_spans().unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].symbol.as_deref(), Some("handle"));
}
