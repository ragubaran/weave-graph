//! FTS5 symbol search (feature `fts`). The index is derived from `nodes`
//! and updated in the transaction that changes a node, so failed reindexes
//! cannot publish divergent FTS rows.
//!
//! `body`/`doc_comment` are a second, independently-managed pair of
//! columns: the automatic per-node write path below only ever populates
//! `symbol_name`/`signature` (it has no source-file access), leaving them
//! empty. The CLI's indexing pass (which does have file access) backfills
//! them afterward via [`upsert_text`] — same "populate now, backfill body
//! text via a separate pass" relationship `vector.rs`'s streaming chunk
//! producer already has to the same node-write path.

use rusqlite::{Connection, OptionalExtension, params};
use weave_graph_core::synonym::split_identifier;
use weave_graph_core::{Node, NodeId, StorageError};

/// Not contentless: verified live that FTS5 refuses a plain `DELETE FROM
/// tbl WHERE rowid = ?` against a `content=''` table ("cannot DELETE from
/// contentless fts5 table") — every delete needs the original column
/// values re-supplied via `INSERT INTO tbl(tbl, rowid, ...) VALUES
/// ('delete', ...)` instead, which `purge_path`/`purge_missing_nodes`
/// can't cheaply provide (they delete by rowid/path, not by known prior
/// content). Contentless mode was proposed and dropped for this reason —
/// see `analysis_im.md` §10 / `proposal_im.md` §5's own follow-up note.
const SYMBOL_FTS_DDL: &str = "CREATE VIRTUAL TABLE symbol_fts USING fts5(
    symbol_name, signature, body, doc_comment,
    tokenize = 'porter unicode61'
)";

fn backend_err(e: rusqlite::Error) -> StorageError {
    StorageError::Backend(e.to_string())
}

fn existing_symbol_fts_sql(conn: &Connection) -> Result<Option<String>, StorageError> {
    conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'symbol_fts'",
        [],
        |row| row.get(0),
    )
    .optional()
    .map_err(backend_err)
}

/// Idempotent: safe to call on every open, whether or not the table
/// already exists. `porter unicode61` gives English stemming for free —
/// no hand-rolled Snowball implementation needed. An existing table whose
/// schema predates the current column set (`body`/`doc_comment`, or the
/// pre-contentless shape) is dropped and rebuilt from `nodes` in place —
/// FTS5 columns are fixed at creation, so a shape change has no `ALTER
/// TABLE` path, only drop-and-rebuild.
pub(crate) fn ensure_fts_table(conn: &Connection) -> Result<(), StorageError> {
    match existing_symbol_fts_sql(conn)? {
        Some(sql) if sql == SYMBOL_FTS_DDL => return Ok(()),
        Some(_) => {
            conn.execute_batch("DROP TABLE symbol_fts;")
                .map_err(backend_err)?;
            conn.execute_batch(&format!("{SYMBOL_FTS_DDL};"))
                .map_err(backend_err)?;
            rebuild(conn)?;
        }
        None => {
            conn.execute_batch(&format!("{SYMBOL_FTS_DDL};"))
                .map_err(backend_err)?;
        }
    }
    Ok(())
}

fn insert_row(
    conn: &Connection,
    id: i64,
    symbol: &str,
    signature: &str,
    body: &str,
    doc_comment: &str,
) -> Result<(), StorageError> {
    conn.execute(
        "INSERT INTO symbol_fts(rowid, symbol_name, signature, body, doc_comment) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, split_identifier(symbol), signature, body, doc_comment],
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
    let mut delete = conn
        .prepare_cached("DELETE FROM symbol_fts WHERE rowid = ?1")
        .map_err(backend_err)?;
    delete.execute(params![id]).map_err(backend_err)?;
    insert_row(conn, id, symbol, signature, "", "")
}

/// Backfills `body`/`doc_comment` for a node whose `symbol_name`/
/// `signature` row already exists (written by [`replace_row`] on the
/// same node earlier in the same reindex). A regular (non-contentless)
/// FTS5 table supports a partial-column `UPDATE` directly — verified
/// live — so this touches only the two new columns, no need to know or
/// re-pass `symbol`/`signature`.
pub(crate) fn upsert_text(
    conn: &Connection,
    id: i64,
    body: &str,
    doc_comment: &str,
) -> Result<(), StorageError> {
    conn.execute(
        "UPDATE symbol_fts SET body = ?2, doc_comment = ?3 WHERE rowid = ?1",
        params![id, body, doc_comment],
    )
    .map_err(backend_err)?;
    Ok(())
}

pub(crate) fn delete_row(conn: &Connection, id: i64) -> Result<(), StorageError> {
    let mut delete = conn
        .prepare_cached("DELETE FROM symbol_fts WHERE rowid = ?1")
        .map_err(backend_err)?;
    delete.execute(params![id]).map_err(backend_err)?;
    Ok(())
}

pub(crate) fn purge_path(conn: &Connection, repo_id: &str, path: &str) -> Result<(), StorageError> {
    let mut delete = conn
        .prepare_cached(
            "DELETE FROM symbol_fts WHERE rowid IN (
                 SELECT id FROM nodes WHERE repo_id = ?1 AND path = ?2
             )",
        )
        .map_err(backend_err)?;
    delete
        .execute(params![repo_id, path])
        .map_err(backend_err)?;
    Ok(())
}

pub(crate) fn purge_missing_nodes(conn: &Connection) -> Result<(), StorageError> {
    let mut delete = conn
        .prepare_cached("DELETE FROM symbol_fts WHERE rowid NOT IN (SELECT id FROM nodes)")
        .map_err(backend_err)?;
    delete.execute([]).map_err(backend_err)?;
    Ok(())
}

/// Reconstructs the derived index for repair or schema migration.
/// Runs FTS5's `optimize` afterward — a bulk operation meant to run
/// rarely, never per-incremental-write, so it belongs only here.
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
        insert_row(conn, id, &symbol, &signature, "", "")?;
    }
    conn.execute("INSERT INTO symbol_fts(symbol_fts) VALUES('optimize')", [])
        .map_err(backend_err)?;
    Ok(())
}

pub(crate) fn search_nodes(
    conn: &Connection,
    match_expr: &str,
    limit: usize,
) -> Result<Vec<Node>, StorageError> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT n.id, n.repo_id, n.path, n.symbol, n.kind, n.line_start, n.line_end, n.signature
             FROM symbol_fts
             JOIN nodes AS n ON n.id = symbol_fts.rowid
             WHERE symbol_fts MATCH ?1
             ORDER BY bm25(symbol_fts, 10.0, 5.0, 1.0, 2.0) LIMIT ?2",
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
