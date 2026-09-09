use super::*;

#[test]
fn fresh_db_migrates_to_latest_version() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), 3);
}

#[test]
fn migrate_is_idempotent_once_current() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    migrate(&conn).unwrap();
    assert_eq!(schema_version(&conn).unwrap(), 3);
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
    assert_eq!(schema_version(&conn).unwrap(), 3);

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
    assert_eq!(schema_version(&conn).unwrap(), 3);

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
        StorageError::SchemaTooNew { found: 999, max: 3 }
    ));
}
