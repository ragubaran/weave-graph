//! M2.7 acceptance: the sqlite crate's incremental-reindex suite —
//! including the Core-Invariant-3 blocking regression test — runs
//! against `TursoStorage` through the shared, factory-parameterized
//! suite.

#[path = "../../weave-graph-store-sqlite/tests/suites/incremental_reindex.rs"]
mod incremental_reindex_suite;

use std::path::Path;

use weave_graph_core::Storage;
use weave_graph_store_turso::TursoStorage;

fn turso_open(path: &Path) -> Box<dyn Storage> {
    Box::new(TursoStorage::open(path).unwrap())
}

#[test]
fn reindex_one_of_two_mutually_referencing_files_leaves_zero_dangling_edges() {
    incremental_reindex_suite::reindex_one_of_two_mutually_referencing_files_leaves_zero_dangling_edges(
        turso_open,
    );
}

#[test]
fn purge_file_edges_removes_edges_in_both_directions() {
    incremental_reindex_suite::purge_file_edges_removes_edges_in_both_directions(turso_open);
}

#[test]
fn unrelated_file_edges_survive_targeted_purge() {
    incremental_reindex_suite::unrelated_file_edges_survive_targeted_purge(turso_open);
}

#[test]
fn bailout_threshold_triggers_on_large_change_fraction() {
    incremental_reindex_suite::bailout_threshold_triggers_on_large_change_fraction();
}

#[test]
fn bailout_floor_protects_tiny_repos() {
    incremental_reindex_suite::bailout_floor_protects_tiny_repos();
}
