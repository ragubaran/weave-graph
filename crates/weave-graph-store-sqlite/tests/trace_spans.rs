//! Integration tests — trace-span persistence.
//!
//! Bodies live in `suites/trace_spans.rs`, shared with the `turso`
//! backend's test target.

#[path = "suites/trace_spans.rs"]
mod trace_spans_suite;

use std::path::Path;

use weave_graph_core::Storage;
use weave_graph_store_sqlite::SqliteStorage;

fn sqlite_open(path: &Path) -> Box<dyn Storage> {
    Box::new(SqliteStorage::open(path).unwrap())
}

#[test]
fn trace_spans_survive_close_and_reopen() {
    trace_spans_suite::trace_spans_survive_close_and_reopen(sqlite_open);
}

#[test]
fn trace_spans_survive_file_purge() {
    trace_spans_suite::trace_spans_survive_file_purge(sqlite_open);
}
