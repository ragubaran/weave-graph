//! Acceptance: the sqlite crate's trace-span suite runs against
//! `TursoStorage` through the shared, factory-parameterized suite.

#[path = "../../weave-graph-store-sqlite/tests/suites/trace_spans.rs"]
mod trace_spans_suite;

use std::path::Path;

use weave_graph_core::Storage;
use weave_graph_store_turso::TursoStorage;

fn turso_open(path: &Path) -> Box<dyn Storage> {
    Box::new(TursoStorage::open(path).unwrap())
}

#[test]
fn trace_spans_survive_close_and_reopen() {
    trace_spans_suite::trace_spans_survive_close_and_reopen(turso_open);
}

#[test]
fn trace_spans_survive_file_purge() {
    trace_spans_suite::trace_spans_survive_file_purge(turso_open);
}
