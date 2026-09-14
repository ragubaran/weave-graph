//! FTS5 symbol search (feature `fts`). The index is derived from `nodes`
//! and updated in the transaction that changes a node, so failed reindexes
//! cannot publish divergent FTS rows.

use rusqlite::{Connection, params};
use weave_graph_core::synonym::split_identifier;
use weave_graph_core::{Node, NodeId, StorageError};

fn backend_err(e: rusqlite::Error) -> StorageError {
    StorageError::Backend(e.to_string())
}

/// Idempotent: safe to call on every open, whether or not the table
/// already exists. `porter unicode61` gives English stemming for free —
/// no hand-rolled Snowball implementation needed.
pub(crate) fn ensure_fts_table(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS symbol_fts USING fts5(
            symbol_name, signature, tokenize = 'porter unicode61'
        );",
    )
    .map_err(backend_err)
}

fn insert_row(
    conn: &Connection,
    id: i64,
    symbol: &str,
    signature: &str,
) -> Result<(), StorageError> {
    conn.execute(
        "INSERT INTO symbol_fts(rowid, symbol_name, signature) VALUES (?1, ?2, ?3)",
        params![id, split_identifier(symbol), signature],
    )
    .map_err(backend_err)?;
    Ok(())
}

pub(crate) fn replace_row(
    conn: &Connection,
    id: i64,
    symbol: &str,
    signature: &str,
) -> Result<(), StorageError> {
    conn.execute("DELETE FROM symbol_fts WHERE rowid = ?1", params![id])
        .map_err(backend_err)?;
    insert_row(conn, id, symbol, signature)
}

pub(crate) fn delete_row(conn: &Connection, id: i64) -> Result<(), StorageError> {
    conn.execute("DELETE FROM symbol_fts WHERE rowid = ?1", params![id])
        .map_err(backend_err)?;
    Ok(())
}

pub(crate) fn purge_path(conn: &Connection, repo_id: &str, path: &str) -> Result<(), StorageError> {
    conn.execute(
        "DELETE FROM symbol_fts WHERE rowid IN (
             SELECT id FROM nodes WHERE repo_id = ?1 AND path = ?2
         )",
        params![repo_id, path],
    )
    .map_err(backend_err)?;
    Ok(())
}

pub(crate) fn purge_missing_nodes(conn: &Connection) -> Result<(), StorageError> {
    conn.execute(
        "DELETE FROM symbol_fts WHERE rowid NOT IN (SELECT id FROM nodes)",
        [],
    )
    .map_err(backend_err)?;
    Ok(())
}

/// Reconstructs the derived index for repair or schema migration.
pub(crate) fn rebuild(conn: &Connection) -> Result<(), StorageError> {
    conn.execute("DELETE FROM symbol_fts", [])
        .map_err(backend_err)?;
    let mut select = conn
        .prepare("SELECT id, symbol, signature FROM nodes")
        .map_err(backend_err)?;
    let rows = select
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(backend_err)?;
    for row in rows {
        let (id, symbol, signature) = row.map_err(backend_err)?;
        insert_row(conn, id, &symbol, &signature)?;
    }
    Ok(())
}

pub(crate) fn search_nodes(
    conn: &Connection,
    match_expr: &str,
    limit: usize,
) -> Result<Vec<Node>, StorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT n.id, n.repo_id, n.path, n.symbol, n.kind, n.line_start, n.line_end, n.signature
             FROM symbol_fts
             JOIN nodes AS n ON n.id = symbol_fts.rowid
             WHERE symbol_fts MATCH ?1
             ORDER BY bm25(symbol_fts, 10.0, 5.0) LIMIT ?2",
        )
        .map_err(backend_err)?;
    stmt.query_map(params![match_expr, limit as i64], |row| {
        Ok(Node {
            id: row.get::<_, i64>(0)? as NodeId,
            repo_id: row.get(1)?,
            path: row.get(2)?,
            symbol: row.get(3)?,
            kind: row.get(4)?,
            line_start: row.get::<_, i64>(5)? as u32,
            line_end: row.get::<_, i64>(6)? as u32,
            signature: row.get(7)?,
        })
    })
    .map_err(backend_err)?
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(backend_err)
}

#[cfg(test)]
mod tests;
