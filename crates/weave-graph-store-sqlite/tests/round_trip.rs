#[path = "suites/round_trip.rs"]
mod round_trip_suite;

use std::path::Path;

use weave_graph_core::Storage;
use weave_graph_store_sqlite::SqliteStorage;

fn sqlite_open(path: &Path) -> Box<dyn Storage> {
    Box::new(SqliteStorage::open(path).unwrap())
}

#[test]
fn graph_survives_close_and_reopen() {
    round_trip_suite::graph_survives_close_and_reopen(sqlite_open);
}
