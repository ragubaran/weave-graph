//! CORE-01 investigation (`docs/phase3_issues.md`): a single process can
//! never safely hold both an open `weave-graph-store-sqlite` (rusqlite,
//! `bundled`) and an open `weave-graph-store-turso` (libsql, `core`)
//! connection — both statically link their own vendored `sqlite3.c`, and
//! the second library to actually open a connection panics on libsql's own
//! threading-config self-check. This is a real, reproducible constraint,
//! not a config problem on this machine — see the panic message before
//! assuming a `StorageBackend` enum can pick between them at runtime
//! inside the one `weave` binary.

use weave_graph_core::{Node, Storage};

#[test]
#[should_panic(expected = "libsql was configured with an incorrect threading configuration")]
fn opening_turso_after_sqlite_in_the_same_process_panics() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut sqlite =
            weave_graph_store_sqlite::SqliteStorage::open(&dir.path().join("a.db")).unwrap();
        sqlite
            .upsert_node(&Node {
                id: 0,
                repo_id: "r".into(),
                path: "a.rs".into(),
                symbol: "foo".into(),
                kind: "function".into(),
                line_start: 1,
                line_end: 2,
                signature: "fn foo()".into(),
            })
            .unwrap();
    }
    // Different file, in-memory even — the conflict is process-global
    // (both libraries' bundled `sqlite3_initialize`/threading state),
    // not about sharing one database file.
    let _ = weave_graph_store_turso::TursoStorage::open_in_memory().unwrap();
}
