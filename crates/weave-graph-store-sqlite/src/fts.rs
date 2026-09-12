//! FTS5 symbol search (`impl.md` M3.7 Tier 1, feature `fts`). Kept out of
//! `weave_graph_core::schema::MIGRATIONS` deliberately: it's a derived
//! index rebuilt wholesale from `nodes`, the same "SQL is authoritative,
//! this is rebuilt from it" relationship the CSR graph already has (M1.3).

use rusqlite::{Connection, params};
use weave_graph_core::synonym::split_identifier;
use weave_graph_core::{NodeId, StorageError};

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

/// Rebuilds the index from every current `nodes` row. Called after a bulk
/// node write inside the same transaction — cheap enough at this repo's
/// measured scale (~2.5k nodes in ~2ms) that a full rebuild beats tracking
/// per-row FTS deltas through every reindex path.
pub(crate) fn rebuild(conn: &Connection) -> Result<(), StorageError> {
    conn.execute("DELETE FROM symbol_fts", [])
        .map_err(backend_err)?;
    let mut select = conn
        .prepare("SELECT id, symbol, signature FROM nodes")
        .map_err(backend_err)?;
    let mut insert = conn
        .prepare("INSERT INTO symbol_fts(rowid, symbol_name, signature) VALUES (?1, ?2, ?3)")
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
        insert
            .execute(params![id, split_identifier(&symbol), signature])
            .map_err(backend_err)?;
    }
    Ok(())
}

/// Ranked node ids for an FTS5 `MATCH` expression (`synonym::expand_query`
/// builds it) — best match first, capped at `limit`.
pub(crate) fn search(
    conn: &Connection,
    match_expr: &str,
    limit: usize,
) -> Result<Vec<NodeId>, StorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT rowid FROM symbol_fts WHERE symbol_fts MATCH ?1 \
             ORDER BY bm25(symbol_fts, 10.0, 5.0) LIMIT ?2",
        )
        .map_err(backend_err)?;
    stmt.query_map(params![match_expr, limit as i64], |row| {
        row.get::<_, i64>(0)
    })
    .map_err(backend_err)?
    .map(|r| r.map(|id| id as NodeId).map_err(backend_err))
    .collect()
}

#[cfg(test)]
mod tests;
