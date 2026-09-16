use weave_graph_core::StorageError;
use weave_graph_core::schema::{LATEST_SCHEMA_VERSION, migrations_after};

fn backend_err(e: libsql::Error) -> StorageError {
    StorageError::Backend(e.to_string())
}

fn current_version(conn: &libsql::Connection) -> Result<u32, StorageError> {
    futures::executor::block_on(async {
        // Fresh database: the table itself doesn't exist yet → version 0.
        // Two queries — the version query must not be prepared against a
        // table that doesn't exist yet.
        let mut rows = conn
            .query(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' \
                 AND name = 'schema_version')",
                (),
            )
            .await?;
        let exists = match rows.next().await? {
            Some(row) => row.get::<i64>(0)? == 1,
            None => false,
        };
        if !exists {
            return Ok(0);
        }
        let mut rows = conn
            .query("SELECT COALESCE(MAX(version), 0) FROM schema_version", ())
            .await?;
        match rows.next().await? {
            Some(row) => Ok(row.get::<i64>(0)? as u32),
            None => Ok(0),
        }
    })
    .map_err(backend_err)
}

/// Replays `weave-graph-core::schema`'s `MIGRATIONS` verbatim over libSQL —
/// libSQL's embedded engine is a SQLite fork, so the DDL is portable as-is
/// and there is one source of truth for the schema. Same rule as the
/// rusqlite driver: each migration in its own transaction, refuse a
/// database newer than this binary knows about.
pub(crate) fn migrate(conn: &libsql::Connection) -> Result<(), StorageError> {
    let current = current_version(conn)?;
    if current > LATEST_SCHEMA_VERSION {
        return Err(StorageError::SchemaTooNew {
            found: current,
            max: LATEST_SCHEMA_VERSION,
        });
    }
    for (version, sql) in migrations_after(current) {
        let batch = format!(
            "BEGIN;\n{sql}\nINSERT INTO schema_version (version, applied_at) VALUES ({version}, strftime('%s', 'now'));\nCOMMIT;"
        );
        futures::executor::block_on(conn.execute_batch(&batch)).map_err(backend_err)?;
    }
    Ok(())
}

pub(crate) fn schema_version(conn: &libsql::Connection) -> Result<u32, StorageError> {
    current_version(conn)
}
