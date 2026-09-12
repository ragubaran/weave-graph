//! Distributed trace spans overlaid onto the static graph (`impl.md`
//! M3.3, `plan.md` §3.3). The struct and its aggregation are unconditional
//! here (the `trace_spans` table exists in every schema, M2.10's
//! precedent); the `otel` Cargo feature gates only the CLI surface
//! (`traces import`, the `latency()` query), never the data model.
//!
//! Spans are keyed by `(trace_id, span_id)` and matched to nodes by
//! symbol at *query* time, never by stored node id — a reindex can
//! renumber ids, and a dangling span pointer is exactly the defect Core
//! Invariant 3 exists to prevent.

/// One imported span, flattened from whatever wire format the importer
/// understood (v1: OTLP JSON). `status_code` is free-form ("Ok", "Error",
/// "Unset") like `Edge::kind` — vendors disagree on spelling.
#[derive(Debug, Clone, PartialEq)]
pub struct TraceSpan {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub service: Option<String>,
    pub name: String,
    /// The graph symbol this span was matched to, if the importer resolved
    /// one (`code.function` attribute, else the span name itself).
    pub symbol: Option<String>,
    pub path: Option<String>,
    pub start_us: i64,
    pub duration_us: i64,
    pub status_code: String,
}

impl TraceSpan {
    pub fn is_error(&self) -> bool {
        self.status_code.eq_ignore_ascii_case("error")
    }
}

/// Latency summary for one symbol's spans. Percentiles are nearest-rank
/// over the sorted duration list — deterministic, no interpolation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanStats {
    pub count: usize,
    pub error_count: usize,
    pub total_us: i64,
    pub min_us: i64,
    pub max_us: i64,
    pub p50_us: i64,
    pub p95_us: i64,
    pub p99_us: i64,
}

fn nearest_rank(sorted: &[i64], percentile: f64) -> i64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((percentile / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[(rank.max(1) - 1).min(sorted.len() - 1)]
}

/// Aggregates the given spans. Empty input yields a zeroed [`SpanStats`]
/// rather than an error — "no spans matched" is a normal query result.
pub fn aggregate(spans: &[TraceSpan]) -> SpanStats {
    let mut durations: Vec<i64> = spans.iter().map(|s| s.duration_us).collect();
    durations.sort_unstable();
    SpanStats {
        count: durations.len(),
        error_count: spans.iter().filter(|s| s.is_error()).count(),
        total_us: durations.iter().sum(),
        min_us: durations.first().copied().unwrap_or(0),
        max_us: durations.last().copied().unwrap_or(0),
        p50_us: nearest_rank(&durations, 50.0),
        p95_us: nearest_rank(&durations, 95.0),
        p99_us: nearest_rank(&durations, 99.0),
    }
}

#[cfg(test)]
mod tests;
