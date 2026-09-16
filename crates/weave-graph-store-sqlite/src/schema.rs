use rusqlite::Connection;
use weave_graph_core::StorageError;
use weave_graph_core::schema::{MIGRATIONS, migrations_after};

fn backend_err(e: rusqlite::Error) -> StorageError {
    StorageError::Backend(e.to_string())
}

fn current_version(conn: &Connection) -> rusqlite::Result<u32> {
    let table_exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_version')",
        [],
        |row| row.get(0),
    )?;
    if !table_exists {
        return Ok(0);
    }
    let version: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |row| row.get(0),
    )?;
    Ok(version as u32)
}

/// Applies every migration newer than the database's current version, each
/// in its own transaction, and refuses to touch a database whose recorded
/// version is newer than this binary knows about. Safe to call repeatedly:
/// once the database is at the latest version, this is a no-op.
pub(crate) fn migrate(conn: &Connection) -> Result<(), StorageError> {
    let current = current_version(conn).map_err(backend_err)?;
    let max = MIGRATIONS
        .iter()
        .map(|(v, _)| *v)
        .max()
        .ok_or_else(|| StorageError::Backend("MIGRATIONS is empty".to_string()))?;
    if current > max {
        return Err(StorageError::SchemaTooNew {
            found: current,
            max,
        });
    }
    for (version, sql) in migrations_after(current) {
        let batch = format!(
            "BEGIN;\n{sql}\nINSERT INTO schema_version (version, applied_at) VALUES ({version}, strftime('%s', 'now'));\nCOMMIT;"
        );
        conn.execute_batch(&batch).map_err(backend_err)?;
    }
    Ok(())
}

pub(crate) fn schema_version(conn: &Connection) -> Result<u32, StorageError> {
    current_version(conn).map_err(backend_err)
}

/// Refuses a database whose recorded version is newer than this binary
/// knows about, same rule as `migrate`, but never writes — a read-only
/// connection (`SqliteStorage::open_read_only`) can't run migrations at all.
pub(crate) fn ensure_not_newer_than_supported(conn: &Connection) -> Result<(), StorageError> {
    let current = current_version(conn).map_err(backend_err)?;
    let max = MIGRATIONS
        .iter()
        .map(|(v, _)| *v)
        .max()
        .ok_or_else(|| StorageError::Backend("MIGRATIONS is empty".to_string()))?;
    if current > max {
        return Err(StorageError::SchemaTooNew {
            found: current,
            max,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
