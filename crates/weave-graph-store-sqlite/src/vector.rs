//! `sqlite-vec` Tier 2 semantic search (`impl.md` M3.7 Tier 2, feature
//! `vector`): binary-quantized ANN candidates reranked by int8 distance,
//! per `docs/vector-proposal.md` §10.4's revised three-stage funnel.
//! Float32 embeddings are never persisted — only ever a transient bound
//! parameter, immediately quantized by `sqlite-vec`'s own SQL functions.

use std::sync::Once;

use rusqlite::{Connection, params, params_from_iter};
use weave_graph_core::embedding::EmbeddingProvider;
use weave_graph_core::{NodeId, StorageError};

/// Fixed to match `MockEmbeddingProvider`'s default — `vec0`'s column
/// dimension is baked into `CREATE VIRTUAL TABLE`, so a different
/// embedding width needs a schema change, not a runtime parameter.
const DIMENSIONS: usize = 384;

fn backend_err(e: rusqlite::Error) -> StorageError {
    StorageError::Backend(e.to_string())
}

static REGISTER_EXTENSION: Once = Once::new();

/// Registers `sqlite-vec`'s C extension once per process via
/// `sqlite3_auto_extension` — every `Connection` opened afterward (in
/// this process) gets `vec0`/`vec_quantize_*` for free.
///
/// Safety: `sqlite_vec::sqlite3_vec_init` is a valid, ABI-compatible
/// SQLite extension entry point — this is `sqlite-vec`'s own documented
/// registration incantation (its crate-level test uses the identical
/// cast), verified live against this exact dependency version before
/// landing here. SQLite itself deduplicates repeat registration of the
/// same pointer, making the `Once` a belt-and-suspenders guard.
///
/// Must run before the *first* `Connection::open*` call in the process —
/// `sqlite3_auto_extension` only affects connections opened after
/// registration, never ones already open (caught live: calling this from
/// inside `ensure_vector_table`, after the connection it was given had
/// already been opened, left every `vec0` query failing with "no such
/// module").
#[allow(unsafe_code)]
pub(crate) fn ensure_vector_extension() {
    REGISTER_EXTENSION.call_once(|| unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut i8,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> i32,
        >(
            sqlite_vec::sqlite3_vec_init as *const ()
        )));
    });
}

pub(crate) fn ensure_vector_table(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS vec_chunks USING vec0(
            binary_vec bit[{DIMENSIONS}],
            int8_vec int8[{DIMENSIONS}]
        );"
    ))
    .map_err(backend_err)
}

fn embedding_bytes(embedder: &dyn EmbeddingProvider, text: &str) -> Vec<u8> {
    embedder
        .embed(text)
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect()
}

/// Rebuilds `vec_chunks` from `chunks` — `(node_id, chunk_text)` pairs the
/// caller already built from source file spans (this module owns no file
/// I/O). Same "derived index, rebuilt wholesale" relationship `fts.rs`
/// already has to `nodes`.
pub(crate) fn rebuild(
    conn: &Connection,
    embedder: &dyn EmbeddingProvider,
    chunks: &[(NodeId, String)],
) -> Result<(), StorageError> {
    conn.execute("DELETE FROM vec_chunks", [])
        .map_err(backend_err)?;
    let mut insert = conn
        .prepare(
            "INSERT INTO vec_chunks(rowid, binary_vec, int8_vec) \
             VALUES (?1, vec_quantize_binary(?2), vec_quantize_int8(?2, 'unit'))",
        )
        .map_err(backend_err)?;
    for (id, text) in chunks {
        let bytes = embedding_bytes(embedder, text);
        insert.execute(params![*id, bytes]).map_err(backend_err)?;
    }
    Ok(())
}

pub(crate) fn rebuild_streaming(
    conn: &Connection,
    embedder: &dyn EmbeddingProvider,
    produce: impl FnOnce(
        &mut dyn FnMut(NodeId, &str) -> Result<(), StorageError>,
    ) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    conn.execute("DELETE FROM vec_chunks", [])
        .map_err(backend_err)?;
    let mut insert = conn
        .prepare(
            "INSERT INTO vec_chunks(rowid, binary_vec, int8_vec) \
             VALUES (?1, vec_quantize_binary(?2), vec_quantize_int8(?2, 'unit'))",
        )
        .map_err(backend_err)?;
    let mut insert_chunk = |id: NodeId, text: &str| {
        let bytes = embedding_bytes(embedder, text);
        insert.execute(params![id, bytes]).map_err(backend_err)?;
        Ok(())
    };
    produce(&mut insert_chunk)
}

pub(crate) fn purge_excluded_paths(
    conn: &Connection,
    excluded_paths: &[String],
) -> Result<u64, StorageError> {
    let mut deleted = 0;
    for prefix in excluded_paths {
        let like_pattern = format!("{prefix}/%");
        deleted += conn
            .execute(
                "DELETE FROM vec_chunks WHERE rowid IN (\
                 SELECT id FROM nodes WHERE path = ?1 OR ?2 = '' OR path LIKE ?3\
                 )",
                params![prefix, prefix, like_pattern],
            )
            .map_err(backend_err)?;
    }
    Ok(deleted as u64)
}

/// Three-stage funnel: binary ANN oversampled by `oversample`, reranked
/// against the int8 column, capped at `limit` — never queries the binary
/// index standalone (recall drops ~7-18% without this rerank per
/// `vector-proposal.md` §10.3's cited research).
///
/// Stage 2 reranks in Rust rather than a second `vec0` KNN query: `WHERE
/// int8_vec MATCH ... AND rowid IN (...)` reliably fails live with "A
/// LIMIT or 'k = ?' constraint is required on vec0 knn queries" — `vec0`
/// doesn't recognize its own KNN constraint once an extra predicate joins
/// it. A plain (non-KNN) row read of the stored `int8_vec` bytes for
/// exactly the stage-1 candidates has no such restriction.
///
/// `visible` (SEC-01) filters candidates *before* `truncate(limit)`, not
/// after — filtering post-truncation starves `limit` of any masked hit
/// ranked ahead of a visible one, sometimes down to zero results, and lets
/// a caller infer a hidden file's existence from the result-count drop.
pub(crate) fn search(
    conn: &Connection,
    embedder: &dyn EmbeddingProvider,
    query_text: &str,
    limit: usize,
    oversample: usize,
    visible: Option<&dyn Fn(NodeId) -> bool>,
) -> Result<Vec<NodeId>, StorageError> {
    let query = embedder.embed(query_text);
    let query_bytes: Vec<u8> = query.iter().flat_map(|f| f.to_le_bytes()).collect();
    let candidate_limit = limit.saturating_mul(oversample).max(limit);

    // `vec0`'s KNN planner needs `LIMIT` as a literal it can see while
    // choosing a query plan — a bound `?` parameter fails the same way
    // (caught live). `candidate_limit` is an internal `usize`, never
    // user-supplied text, so interpolating it is not an injection risk.
    let mut stage1 = conn
        .prepare(&format!(
            "SELECT rowid FROM vec_chunks \
             WHERE binary_vec MATCH vec_quantize_binary(?1) ORDER BY distance LIMIT {candidate_limit}",
        ))
        .map_err(backend_err)?;
    let candidates: Vec<i64> = stage1
        .query_map(params![query_bytes], |r| r.get(0))
        .map_err(backend_err)?
        .collect::<Result<_, _>>()
        .map_err(backend_err)?;
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = candidates.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!("SELECT rowid, int8_vec FROM vec_chunks WHERE rowid IN ({placeholders})");
    let mut stage2 = conn.prepare(&sql).map_err(backend_err)?;
    let mut scored: Vec<(NodeId, f32)> = stage2
        .query_map(params_from_iter(candidates.iter()), |r| {
            Ok((r.get::<_, i64>(0)? as NodeId, r.get::<_, Vec<u8>>(1)?))
        })
        .map_err(backend_err)?
        .map(|row| {
            let (id, int8_bytes) = row.map_err(backend_err)?;
            let dot: f32 = int8_bytes
                .iter()
                .map(|&b| b as i8 as f32)
                .zip(&query)
                .map(|(a, b)| a * b)
                .sum();
            Ok((id, dot))
        })
        .collect::<Result<_, StorageError>>()?;
    if let Some(visible) = visible {
        scored.retain(|(id, _)| visible(*id));
    }
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    scored.truncate(limit);
    Ok(scored.into_iter().map(|(id, _)| id).collect())
}

#[cfg(test)]
mod tests;
