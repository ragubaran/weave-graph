//! `weave traces` imports distributed
//! trace spans from an OTLP/JSON file and overlays them onto the indexed
//! graph. File ingest only — no OTLP collector endpoint, ever, in this
//! process (Core Invariant 1: deterministic, zero network). Export your
//! traces from Jaeger/Datadog as OTLP JSON and point this at the file.
//!
//! Symbol resolution happens at import time (`code.function`, else the
//! span name), but matching against nodes happens at *query* time by
//! symbol string — a reindex can renumber node ids, and a stored id
//! would be exactly the dangling pointer Core Invariant 3 forbids.

use std::path::Path;

use serde_json::Value;
use weave_graph_core::trace::{TraceSpan, aggregate};
use weave_graph_core::{Node, Storage};

pub(crate) fn cmd_traces_import(
    root: &Path,
    file: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(file)
        .map_err(|e| format!("cannot read trace file {}: {e}", file.display()))?;
    let spans = parse_otlp_json(&content)?;
    let (storage, _db) = crate::open_storage_for_read(root)?;
    let nodes = storage.all_nodes()?;
    let mut matched = 0usize;
    for mut span in spans {
        span.symbol = resolve_symbol(&nodes, span.symbol.as_deref(), &span.name);
        if span.symbol.is_some() {
            matched += 1;
        }
        storage.upsert_trace_span(&span)?;
    }
    println!(
        "✓ imported span(s) from {} — {matched} matched to graph symbols",
        file.display()
    );
    println!("Query with: weave query \"latency(<symbol>)\"");
    Ok(())
}

/// OTLP/JSON (v1) trace export: `resourceSpans[].resource.attributes`
/// for `service.name`, `scopeSpans[].spans[]` for the spans themselves.
/// Unknown/missing fields default — a partial export imports the spans
/// it can describe rather than failing whole-file.
pub(crate) fn parse_otlp_json(content: &str) -> Result<Vec<TraceSpan>, String> {
    let root: Value = serde_json::from_str(content).map_err(|e| format!("invalid JSON: {e}"))?;
    let resource_spans = root
        .get("resourceSpans")
        .and_then(Value::as_array)
        .ok_or("no `resourceSpans` array — not an OTLP JSON trace export")?;

    let mut spans = Vec::new();
    for rs in resource_spans {
        let service = attribute(rs.get("resource"), "service.name");
        for scope in rs
            .get("scopeSpans")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            for span in scope
                .get("spans")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or(&[])
            {
                spans.push(parse_span(span, service.clone())?);
            }
        }
    }
    Ok(spans)
}

fn parse_span(span: &Value, service: Option<String>) -> Result<TraceSpan, String> {
    let name = span
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if name.is_empty() {
        return Err("span with no `name`".to_string());
    }
    let start_ns = nanos(span.get("startTimeUnixNano"))?;
    let end_ns = nanos(span.get("endTimeUnixNano"))?;
    let status_code = match span
        .get("status")
        .and_then(|s| s.get("statusCode"))
        .and_then(Value::as_str)
    {
        Some("STATUS_CODE_ERROR") => "Error",
        Some("STATUS_CODE_OK") => "Ok",
        _ => "Unset",
    };
    Ok(TraceSpan {
        trace_id: span
            .get("traceId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        span_id: span
            .get("spanId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        parent_span_id: span
            .get("parentSpanId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        service,
        name,
        symbol: attribute(Some(span), "code.function"),
        path: attribute(Some(span), "code.filepath"),
        start_us: (start_ns / 1_000) as i64,
        duration_us: ((end_ns.saturating_sub(start_ns)) / 1_000) as i64,
        status_code: status_code.to_string(),
    })
}

fn nanos(field: Option<&Value>) -> Result<u128, String> {
    match field {
        Some(Value::String(s)) => s
            .parse::<u128>()
            .map_err(|_| format!("bad unix nano {s:?}")),
        Some(Value::Number(n)) => n
            .as_u64()
            .map(u128::from)
            .ok_or_else(|| "bad unix nano number".to_string()),
        _ => Err("missing startTimeUnixNano/endTimeUnixNano".to_string()),
    }
}

/// One attribute lookup over an OTLP attributes array
/// (`[{key, value: {stringValue}}]`).
fn attribute(container: Option<&Value>, key: &str) -> Option<String> {
    let attrs = container?.get("attributes")?.as_array()?;
    attrs
        .iter()
        .find(|a| a.get("key").and_then(Value::as_str) == Some(key))
        .and_then(|a| {
            a.get("value")
                .and_then(|v| v.get("stringValue"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

/// The symbol a span will be queried by: the first of `code.function` or
/// the span name that names an indexed symbol, else `code.function` if
/// present, else the name. Unresolved spans import anyway — they become
/// queryable once a matching symbol appears in a later index.
fn resolve_symbol(nodes: &[Node], code_function: Option<&str>, name: &str) -> Option<String> {
    for candidate in [code_function, Some(name)] {
        if let Some(c) = candidate.filter(|c| !c.is_empty())
            && nodes.iter().any(|n| n.symbol == c)
        {
            return Some(c.to_string());
        }
    }
    code_function
        .filter(|c| !c.is_empty())
        .map(str::to_string)
        .or_else(|| (!name.is_empty()).then(|| name.to_string()))
}

/// Carries trace spans across a full rebuild (`index.rs`): the rebuild
/// writes a fresh database, so the old one's spans must be explicitly
/// copied — same reason notes carry over. Span→node matching is by
/// symbol at query time, so no re-resolution is needed here.
pub(crate) fn carry_over(
    active_db: &Path,
    rebuild: &dyn Storage,
) -> Result<(), Box<dyn std::error::Error>> {
    let previous = weave_graph_store_sqlite::SqliteStorage::open_read_only(active_db)?;
    for span in previous.all_trace_spans()? {
        rebuild.upsert_trace_span(&span)?;
    }
    Ok(())
}

/// `latency(<symbol>)` query support (`query.rs`): the aggregate stats
/// for one symbol's matched spans. Hidden (`rbac`-masked) symbols never
/// get here — `query.rs` resolves the argument against the masked node
/// list first, so masking is inherited, not re-implemented.
pub(crate) fn latency_text(storage: &dyn Storage, symbol: &str) -> Result<String, String> {
    let spans: Vec<TraceSpan> = storage
        .all_trace_spans()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|s| s.symbol.as_deref() == Some(symbol))
        .collect();
    if spans.is_empty() {
        return Ok(format!("no trace spans matched to symbol {symbol}"));
    }
    let stats = aggregate(&spans);
    Ok(format!(
        "{}: {} span(s), {} error(s)\n  p50 {}µs · p95 {}µs · p99 {}µs · total {}µs (min {}µs, max {}µs)",
        symbol,
        stats.count,
        stats.error_count,
        stats.p50_us,
        stats.p95_us,
        stats.p99_us,
        stats.total_us,
        stats.min_us,
        stats.max_us,
    ))
}

#[cfg(test)]
mod tests;
