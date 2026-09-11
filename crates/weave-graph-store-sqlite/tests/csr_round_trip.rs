#[path = "suites/csr_round_trip.rs"]
mod csr_round_trip_suite;

use std::path::Path;

use weave_graph_core::Storage;
use weave_graph_store_sqlite::SqliteStorage;

fn sqlite_open(path: &Path) -> Box<dyn Storage> {
    Box::new(SqliteStorage::open(path).unwrap())
}

#[test]
fn csr_query_path_matches_sql_query_path_on_the_same_graph() {
    csr_round_trip_suite::csr_query_path_matches_sql_query_path_on_the_same_graph(sqlite_open);
}
