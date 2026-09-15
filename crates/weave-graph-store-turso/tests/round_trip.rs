//! Acceptance: the sqlite crate's round-trip suite runs against
//! `TursoStorage` through the shared, factory-parameterized suite —
//! the same test bodies, a second backend.

#[path = "../../weave-graph-store-sqlite/tests/suites/round_trip.rs"]
mod round_trip_suite;

use std::path::Path;

use weave_graph_core::Storage;
use weave_graph_store_turso::TursoStorage;

fn turso_open(path: &Path) -> Box<dyn Storage> {
    Box::new(TursoStorage::open(path).unwrap())
}

#[test]
fn graph_survives_close_and_reopen() {
    round_trip_suite::graph_survives_close_and_reopen(turso_open);
}
