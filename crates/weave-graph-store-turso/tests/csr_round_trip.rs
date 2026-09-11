//! M2.7 acceptance: the sqlite crate's CSR round-trip suite runs against
//! `TursoStorage` through the shared, factory-parameterized suite.

#[path = "../../weave-graph-store-sqlite/tests/suites/csr_round_trip.rs"]
mod csr_round_trip_suite;

use std::path::Path;

use weave_graph_core::Storage;
use weave_graph_store_turso::TursoStorage;

fn turso_open(path: &Path) -> Box<dyn Storage> {
    Box::new(TursoStorage::open(path).unwrap())
}

#[test]
fn csr_query_path_matches_sql_query_path_on_the_same_graph() {
    csr_round_trip_suite::csr_query_path_matches_sql_query_path_on_the_same_graph(turso_open);
}
