use super::*;

fn span(symbol: Option<&str>, duration_us: i64) -> TraceSpan {
    TraceSpan {
        trace_id: "t1".to_string(),
        span_id: format!("s{duration_us}"),
        parent_span_id: None,
        service: Some("api".to_string()),
        name: symbol.unwrap_or("op").to_string(),
        symbol: symbol.map(str::to_string),
        path: None,
        start_us: 0,
        duration_us,
        status_code: "Ok".to_string(),
    }
}

#[test]
fn percentiles_are_nearest_rank_over_sorted_durations() {
    let spans: Vec<TraceSpan> = [1, 2, 3, 4, 5]
        .iter()
        .map(|d| span(Some("f"), *d))
        .collect();
    let stats = aggregate(&spans);
    assert_eq!(stats.count, 5);
    assert_eq!(stats.total_us, 15);
    assert_eq!(stats.min_us, 1);
    assert_eq!(stats.max_us, 5);
    assert_eq!(stats.p50_us, 3);
    assert_eq!(stats.p95_us, 5);
    assert_eq!(stats.p99_us, 5);
}

#[test]
fn error_spans_are_counted_by_status_code() {
    let mut error = span(Some("f"), 10);
    error.status_code = "Error".to_string();
    let stats = aggregate(&[span(Some("f"), 1), error, span(Some("f"), 2)]);
    assert_eq!(stats.count, 3);
    assert_eq!(stats.error_count, 1);
}

#[test]
fn empty_input_yields_zeroed_stats() {
    let stats = aggregate(&[]);
    assert_eq!(stats.count, 0);
    assert_eq!(stats.total_us, 0);
    assert_eq!(stats.p99_us, 0);
}

#[test]
fn nearest_rank_never_indexes_out_of_bounds() {
    let sorted = [10, 20];
    assert_eq!(nearest_rank(&sorted, 50.0), 10);
    assert_eq!(nearest_rank(&sorted, 95.0), 20);
    assert_eq!(nearest_rank(&sorted, 99.0), 20);
    assert_eq!(nearest_rank(&[], 50.0), 0);
}
