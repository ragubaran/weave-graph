use super::*;
use weave_graph_core::schema::{
    LATEST_SCHEMA_VERSION, V1_CREATE_TABLES, V2_TRAVERSAL_INDICES, V3_DOC_LINK_PROVENANCE,
    V4_NOTES_TABLE, V5_TRACE_SPANS_TABLE, V6_CONTRACT_ENTRIES, V7_RESOLVER_INPUTS,
};

#[test]
fn fresh_db_migrates_to_latest_version() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), LATEST_SCHEMA_VERSION);
}

#[test]
fn migrate_is_idempotent_once_current() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    migrate(&conn).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), LATEST_SCHEMA_VERSION);
}

#[test]
fn legacy_v1_db_upgrades_to_latest() {
    let conn = Connection::open_in_memory().unwrap();
    // Simulate a database created by a binary that only knew about v1.
    let v1_only = format!(
        "BEGIN;\n{V1_CREATE_TABLES}\nINSERT INTO schema_version (version, applied_at) VALUES (1, strftime('%s', 'now'));\nCOMMIT;"
    );
    conn.execute_batch(&v1_only).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), 1);

    migrate(&conn).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), LATEST_SCHEMA_VERSION);

    // The v2 indices must actually exist now, not just the version row.
    let index_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = 'idx_edges_source')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(index_exists);
}

#[test]
fn legacy_v2_db_upgrades_to_v3_doc_link_provenance_columns() {
    let conn = Connection::open_in_memory().unwrap();
    let v2_only = format!(
        "BEGIN;\n{V1_CREATE_TABLES}\n{V2_TRAVERSAL_INDICES}\nINSERT INTO schema_version (version, applied_at) VALUES (2, strftime('%s', 'now'));\nCOMMIT;"
    );
    conn.execute_batch(&v2_only).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), 2);

    migrate(&conn).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), LATEST_SCHEMA_VERSION);

    for column in [
        "provenance_commit",
        "provenance_hash",
        "provenance_signature",
    ] {
        let has_column: bool = conn
            .query_row(
                &format!(
                    "SELECT EXISTS(SELECT 1 FROM pragma_table_info('doc_links') WHERE name = '{column}')"
                ),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(has_column, "doc_links.{column} must exist after v3");
    }
}

#[test]
fn refuses_to_open_a_schema_newer_than_this_binary_supports() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE schema_version (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);
         INSERT INTO schema_version (version, applied_at) VALUES (999, strftime('%s', 'now'));",
    )
    .unwrap();

    let err = migrate(&conn).unwrap_err();
    assert!(matches!(
        err,
        StorageError::SchemaTooNew {
            found: 999,
            max: LATEST_SCHEMA_VERSION
        }
    ));
}

#[test]
fn schema_version_errors_when_the_initial_table_check_query_fails() {
    // A corrupted (non-database) file: `Connection::open` succeeds lazily,
    // but the very first real query against it — `current_version`'s
    // `sqlite_master` check — fails, exercising `backend_err` for real.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("corrupt.db");
    std::fs::write(&path, b"not a valid sqlite database, just garbage bytes").unwrap();
    let conn = Connection::open(&path).unwrap();
    let err = schema_version(&conn).unwrap_err();
    assert!(matches!(err, StorageError::Backend(_)));
}

#[test]
fn schema_version_errors_when_the_schema_version_table_is_malformed() {
    // `schema_version` exists (so the table-exists check passes) but
    // without the `version` column current_version's second query expects.
    let conn = Connection::open_in_memory().unwrap();
    conn.execute("CREATE TABLE schema_version (not_version INTEGER)", [])
        .unwrap();
    let err = schema_version(&conn).unwrap_err();
    assert!(matches!(err, StorageError::Backend(_)));
}

#[test]
fn legacy_v3_db_upgrades_to_v4_notes_table() {
    let conn = Connection::open_in_memory().unwrap();
    let v3_only = format!(
        "BEGIN;\n{V1_CREATE_TABLES}\n{V2_TRAVERSAL_INDICES}\n{V3_DOC_LINK_PROVENANCE}\nINSERT INTO schema_version (version, applied_at) VALUES (3, strftime('%s', 'now'));\nCOMMIT;"
    );
    conn.execute_batch(&v3_only).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), 3);

    migrate(&conn).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), LATEST_SCHEMA_VERSION);

    let notes_table: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'notes')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(notes_table, "notes table must exist after v4");
}

#[test]
fn legacy_v7_db_upgrades_and_removes_the_unused_semantic_key_index() {
    let conn = Connection::open_in_memory().unwrap();
    let v7_only = format!(
        "BEGIN;\n{V1_CREATE_TABLES}\n{V2_TRAVERSAL_INDICES}\n{V3_DOC_LINK_PROVENANCE}\n\
         {V4_NOTES_TABLE}\n{V5_TRACE_SPANS_TABLE}\n{V6_CONTRACT_ENTRIES}\n{V7_RESOLVER_INPUTS}\n\
         INSERT INTO schema_version (version, applied_at) VALUES (7, strftime('%s', 'now'));\nCOMMIT;"
    );
    conn.execute_batch(&v7_only).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), 7);

    migrate(&conn).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), LATEST_SCHEMA_VERSION);

    let has_column: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('nodes') WHERE name = 'semantic_key')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(has_column, "nodes.semantic_key must exist after v8");
    let has_index: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = 'idx_nodes_semantic_key')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !has_index,
        "v9 must remove the unused wide semantic-key index"
    );
}

#[test]
fn ensure_not_newer_than_supported_allows_current_schema() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    ensure_not_newer_than_supported(&conn).unwrap();
}

#[test]
fn ensure_not_newer_than_supported_allows_older_schema() {
    let conn = Connection::open_in_memory().unwrap();
    let v1_only = format!(
        "BEGIN;\n{V1_CREATE_TABLES}\nINSERT INTO schema_version (version, applied_at) VALUES (1, strftime('%s', 'now'));\nCOMMIT;"
    );
    conn.execute_batch(&v1_only).unwrap();
    ensure_not_newer_than_supported(&conn).unwrap();
}

#[test]
fn ensure_not_newer_than_supported_rejects_newer_schema() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE schema_version (version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);
         INSERT INTO schema_version (version, applied_at) VALUES (999, strftime('%s', 'now'));",
    )
    .unwrap();

    let err = ensure_not_newer_than_supported(&conn).unwrap_err();
    assert!(matches!(
        err,
        StorageError::SchemaTooNew {
            found: 999,
            max: LATEST_SCHEMA_VERSION
        }
    ));
}

#[test]
fn migrate_fails_on_sql_error() {
    let conn = Connection::open_in_memory().unwrap();
    // Simulate a v1 database
    let v1_only = format!(
        "BEGIN;\n{V1_CREATE_TABLES}\nINSERT INTO schema_version (version, applied_at) VALUES (1, strftime('%s', 'now'));\nCOMMIT;"
    );
    conn.execute_batch(&v1_only).unwrap();

    // Create an index that V2 will try to create, causing a conflict
    conn.execute_batch("CREATE INDEX idx_edges_source ON nodes(id);")
        .unwrap();

    let err = migrate(&conn).unwrap_err();
    assert!(matches!(err, StorageError::Backend(_)));
}

#[test]
fn migrate_fails_on_version_insert() {
    let conn = Connection::open_in_memory().unwrap();
    let v1_only = format!(
        "BEGIN;\n{V1_CREATE_TABLES}\nINSERT INTO schema_version (version, applied_at) VALUES (1, strftime('%s', 'now'));\nCOMMIT;"
    );
    conn.execute_batch(&v1_only).unwrap();

    conn.execute_batch("CREATE TRIGGER block_insert BEFORE INSERT ON schema_version BEGIN SELECT RAISE(ABORT, 'blocked'); END;").unwrap();

    let err = migrate(&conn).unwrap_err();
    assert!(matches!(err, StorageError::Backend(_)));
}

#[test]
fn migrate_fails_if_already_in_transaction() {
    let conn = Connection::open_in_memory().unwrap();
    let v1_only = format!(
        "BEGIN;\n{V1_CREATE_TABLES}\nINSERT INTO schema_version (version, applied_at) VALUES (1, strftime('%s', 'now'));\nCOMMIT;"
    );
    conn.execute_batch(&v1_only).unwrap();

    conn.execute_batch("BEGIN TRANSACTION;").unwrap();
    let err = migrate(&conn).unwrap_err();
    assert!(matches!(err, StorageError::Backend(_)));
}
