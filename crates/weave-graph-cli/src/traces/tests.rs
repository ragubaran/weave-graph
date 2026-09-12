use super::*;

const OTLP: &str = r#"{
  "resourceSpans": [{
    "resource": {"attributes": [{"key": "service.name", "value": {"stringValue": "api"}}]},
    "scopeSpans": [{
      "spans": [
        {
          "traceId": "aaaa", "spanId": "s1", "name": "handle",
          "startTimeUnixNano": "1000000000", "endTimeUnixNano": "1003000000",
          "status": {"statusCode": "STATUS_CODE_OK"},
          "attributes": [{"key": "code.function", "value": {"stringValue": "handle"}}]
        },
        {
          "traceId": "aaaa", "spanId": "s2", "parentSpanId": "s1", "name": "save",
          "startTimeUnixNano": "1001000000", "endTimeUnixNano": "1009000000",
          "status": {"statusCode": "STATUS_CODE_ERROR"}
        }
      ]
    }]
  }]
}"#;

#[test]
fn otlp_json_parses_spans_with_attributes_and_status() {
    let spans = parse_otlp_json(OTLP).unwrap();
    assert_eq!(spans.len(), 2);

    let first = &spans[0];
    assert_eq!(first.name, "handle");
    assert_eq!(first.service.as_deref(), Some("api"));
    assert_eq!(first.symbol.as_deref(), Some("handle"));
    assert_eq!(first.duration_us, 3_000);
    assert_eq!(first.status_code, "Ok");
    assert!(!first.is_error());

    let second = &spans[1];
    assert_eq!(second.name, "save");
    assert_eq!(second.parent_span_id.as_deref(), Some("s1"));
    assert_eq!(second.symbol, None);
    assert_eq!(second.duration_us, 8_000);
    assert_eq!(second.status_code, "Error");
    assert!(second.is_error());
}

#[test]
fn otlp_json_rejects_non_trace_documents() {
    assert!(parse_otlp_json("{}").is_err());
    assert!(parse_otlp_json("not json").is_err());
}

#[test]
fn otlp_json_accepts_numeric_nano_timestamps() {
    let doc = r#"{"resourceSpans":[{"scopeSpans":[{"spans":[{"name":"f","startTimeUnixNano":1000,"endTimeUnixNano":4000}]}]}]}"#;
    let spans = parse_otlp_json(doc).unwrap();
    assert_eq!(spans[0].duration_us, 3);
}

#[test]
fn resolve_symbol_prefers_code_function_then_span_name() {
    let nodes = vec![node("handle"), node("save")];
    assert_eq!(
        resolve_symbol(&nodes, Some("handle"), "irrelevant"),
        Some("handle".to_string())
    );
    assert_eq!(
        resolve_symbol(&nodes, None, "save"),
        Some("save".to_string())
    );
    assert_eq!(
        resolve_symbol(&nodes, None, "unknown"),
        Some("unknown".to_string())
    );
    assert_eq!(
        resolve_symbol(&nodes, Some("ghost"), "unknown"),
        Some("ghost".to_string())
    );
    assert_eq!(resolve_symbol(&nodes, Some(""), ""), None);
}

fn node(symbol: &str) -> Node {
    Node {
        id: 0,
        repo_id: "local".into(),
        path: "src/lib.rs".into(),
        symbol: symbol.into(),
        kind: "function".into(),
        line_start: 1,
        line_end: 2,
        signature: symbol.into(),
    }
}

#[test]
fn latency_text_aggregates_only_the_named_symbol() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("graph.db");
    let storage = weave_graph_store_sqlite::SqliteStorage::open(&db).unwrap();
    for (symbol, duration) in [("handle", 100), ("handle", 300), ("other", 9_999)] {
        storage
            .upsert_trace_span(&TraceSpan {
                trace_id: "t".into(),
                span_id: format!("s{symbol}{duration}"),
                parent_span_id: None,
                service: None,
                name: symbol.into(),
                symbol: Some(symbol.into()),
                path: None,
                start_us: 0,
                duration_us: duration,
                status_code: "Ok".into(),
            })
            .unwrap();
    }
    let text = latency_text(&storage, "handle").unwrap();
    assert!(text.contains("handle: 2 span(s)"), "{text}");
    assert!(text.contains("p50 100"), "{text}");
    assert!(text.contains("p99 300"), "{text}");
    assert!(!text.contains("other"));

    assert_eq!(
        latency_text(&storage, "nospans").unwrap(),
        "no trace spans matched to symbol nospans"
    );
}
