//! `sqlite-vec` semantic search (feature `vector`): binary-quantized ANN
//! candidates reranked by int8 distance in a three-stage funnel.
//! Float32 embeddings are never persisted — only ever a transient bound
//! parameter, immediately quantized by `sqlite-vec`'s own SQL functions.

use std::sync::Once;

use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
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
        );
        CREATE TABLE IF NOT EXISTS vector_metadata (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            model_id TEXT NOT NULL
        );"
    ))
    .map_err(backend_err)
}

fn embedding_bytes(embedder: &dyn EmbeddingProvider, text: &str) -> Result<Vec<u8>, StorageError> {
    embedder
        .embed(text)
        .map_err(|err| StorageError::Backend(err.to_string()))
        .map(|embedding| embedding.iter().flat_map(|f| f.to_le_bytes()).collect())
}

fn validate_dimensions(embedder: &dyn EmbeddingProvider) -> Result<(), StorageError> {
    if embedder.dimensions() == DIMENSIONS {
        Ok(())
    } else {
        Err(StorageError::Backend(format!(
            "vector dimensions {} do not match index dimensions {DIMENSIONS}",
            embedder.dimensions()
        )))
    }
}

fn write_model_id(conn: &Connection, embedder: &dyn EmbeddingProvider) -> Result<(), StorageError> {
    conn.execute(
        "INSERT INTO vector_metadata(singleton, model_id) VALUES (1, ?1) \
         ON CONFLICT(singleton) DO UPDATE SET model_id = excluded.model_id",
        [embedder.fingerprint()],
    )
    .map_err(backend_err)?;
    Ok(())
}

fn validate_model_id(
    conn: &Connection,
    embedder: &dyn EmbeddingProvider,
) -> Result<(), StorageError> {
    let stored = conn
        .query_row(
            "SELECT model_id FROM vector_metadata WHERE singleton = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(backend_err)?;
    match stored {
        Some(model_id) if model_id == embedder.fingerprint() => Ok(()),
        Some(model_id) => Err(StorageError::Backend(format!(
            "vector index was built with {model_id}; rebuild it with {}",
            embedder.fingerprint()
        ))),
        None => Err(StorageError::Backend(
            "vector index has no model identity; run a full reindex".to_string(),
        )),
    }
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
    validate_dimensions(embedder)?;
    conn.execute("DELETE FROM vec_chunks", [])
        .map_err(backend_err)?;
    let mut insert = conn
        .prepare(
            "INSERT INTO vec_chunks(rowid, binary_vec, int8_vec) \
             VALUES (?1, vec_quantize_binary(?2), vec_quantize_int8(?2, 'unit'))",
        )
        .map_err(backend_err)?;
    for (id, text) in chunks {
        let bytes = embedding_bytes(embedder, text)?;
        insert.execute(params![*id, bytes]).map_err(backend_err)?;
    }
    write_model_id(conn, embedder)?;
    Ok(())
}

pub(crate) fn rebuild_streaming(
    conn: &Connection,
    embedder: &dyn EmbeddingProvider,
    produce: impl FnOnce(
        &mut dyn FnMut(NodeId, &str) -> Result<(), StorageError>,
    ) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    validate_dimensions(embedder)?;
    conn.execute("DELETE FROM vec_chunks", [])
        .map_err(backend_err)?;
    let mut insert = conn
        .prepare(
            "INSERT INTO vec_chunks(rowid, binary_vec, int8_vec) \
             VALUES (?1, vec_quantize_binary(?2), vec_quantize_int8(?2, 'unit'))",
        )
        .map_err(backend_err)?;
    let mut insert_chunk = |id: NodeId, text: &str| {
        let bytes = embedding_bytes(embedder, text)?;
        insert.execute(params![id, bytes]).map_err(backend_err)?;
        Ok(())
    };
    produce(&mut insert_chunk)?;
    write_model_id(conn, embedder)
}

pub(crate) fn upsert_streaming(
    conn: &Connection,
    embedder: &dyn EmbeddingProvider,
    produce: impl FnOnce(
        &mut dyn FnMut(NodeId, &str) -> Result<(), StorageError>,
    ) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    validate_dimensions(embedder)?;
    validate_model_id(conn, embedder)?;
    let mut delete = conn
        .prepare("DELETE FROM vec_chunks WHERE rowid = ?1")
        .map_err(backend_err)?;
    let mut insert = conn
        .prepare(
            "INSERT INTO vec_chunks(rowid, binary_vec, int8_vec) \
             VALUES (?1, vec_quantize_binary(?2), vec_quantize_int8(?2, 'unit'))",
        )
        .map_err(backend_err)?;
    let mut upsert_chunk = |id: NodeId, text: &str| {
        delete.execute(params![id]).map_err(backend_err)?;
        let bytes = embedding_bytes(embedder, text)?;
        insert.execute(params![id, bytes]).map_err(backend_err)?;
        Ok(())
    };
    produce(&mut upsert_chunk)
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
/// index standalone, since recall drops sharply without this rerank.
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
    validate_dimensions(embedder)?;
    let has_vectors: bool = conn
        .query_row("SELECT EXISTS(SELECT 1 FROM vec_chunks)", [], |row| {
            row.get(0)
        })
        .map_err(backend_err)?;
    if !has_vectors {
        return Ok(Vec::new());
    }
    validate_model_id(conn, embedder)?;
    let query = embedder
        .embed_query(query_text)
        .map_err(|err| StorageError::Backend(err.to_string()))?;
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

/// `vec_quantize_int8(_, 'unit')` linearly maps a unit-normalized float
/// component in `[-1, 1]` to an `i8` in `[-127, 127]`. Two quantized unit
/// vectors' raw dot product is therefore scaled by `127 * 127` relative
/// to their real cosine similarity — dividing by this constant is what
/// makes [`find_similar_pairs`]'s scores comparable to a plain
/// `threshold: 0.85`-style cosine value in `.weave/policy.yaml`, not an
/// internal quantization detail the config author has to know about.
const INT8_UNIT_SCALE: f32 = 127.0 * 127.0;

/// POL-02: self-KNN over every chunk already in `scope_ids` — reuses
/// [`search`]'s own two-stage binary-ANN-then-int8-rerank funnel, but the
/// query vector for each stage-1 lookup is that node's own stored
/// `binary_vec`, not a freshly embedded text query. No `EmbeddingProvider`
/// is needed at all: only vectors already persisted by a prior
/// `rebuild_vector_index` are read. Pairs are deduped so `(a, b)` and
/// `(b, a)` collapse to one entry with a stable `a < b` ordering.
pub(crate) fn find_similar_pairs(
    conn: &Connection,
    scope_ids: &[NodeId],
    threshold: f32,
    oversample: usize,
) -> Result<Vec<(NodeId, NodeId, f32)>, StorageError> {
    if scope_ids.is_empty() {
        return Ok(Vec::new());
    }
    let has_vectors: bool = conn
        .query_row("SELECT EXISTS(SELECT 1 FROM vec_chunks)", [], |row| {
            row.get(0)
        })
        .map_err(backend_err)?;
    if !has_vectors {
        return Ok(Vec::new());
    }

    // +1 reserves the slot `vec0` always spends on the node's own
    // guaranteed zero-distance self-match (never excludable in the SQL
    // itself, see the stage-1 comment below) — without it, `oversample`'s
    // worth of *other* candidates would silently shrink by one.
    let candidate_limit = oversample.max(1) + 1;
    let mut own_int8_stmt = conn
        .prepare("SELECT int8_vec FROM vec_chunks WHERE rowid = ?1")
        .map_err(backend_err)?;
    // `vec0`'s KNN planner needs `LIMIT` as a literal, and — like
    // `search`'s own stage-1 query — refuses any extra predicate
    // alongside the `MATCH`/`ORDER BY`/`LIMIT` triple ("A LIMIT or 'k = ?'
    // constraint is required on vec0 knn queries", caught live). So the
    // query can't exclude the node's own id via `AND rowid != ?1`; the
    // self-match (distance 0, always present and always ranked first) is
    // filtered out in Rust below instead.
    let mut stage1 = conn
        .prepare(&format!(
            "SELECT rowid FROM vec_chunks \
             WHERE binary_vec MATCH (SELECT binary_vec FROM vec_chunks WHERE rowid = ?1) \
             ORDER BY distance LIMIT {candidate_limit}",
        ))
        .map_err(backend_err)?;

    let mut seen: std::collections::HashSet<(NodeId, NodeId)> = std::collections::HashSet::new();
    let mut pairs = Vec::new();
    for &id in scope_ids {
        let own_int8: Option<Vec<u8>> = own_int8_stmt
            .query_row(params![id], |r| r.get(0))
            .optional()
            .map_err(backend_err)?;
        let Some(own_int8) = own_int8 else {
            continue;
        };
        let candidates: Vec<i64> = stage1
            .query_map(params![id], |r| r.get(0))
            .map_err(backend_err)?
            .collect::<Result<_, _>>()
            .map_err(backend_err)?;
        let candidates: Vec<i64> = candidates
            .into_iter()
            .filter(|&cand| cand as NodeId != id)
            .collect();
        if candidates.is_empty() {
            continue;
        }
        let placeholders = candidates.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("SELECT rowid, int8_vec FROM vec_chunks WHERE rowid IN ({placeholders})");
        let mut stage2 = conn.prepare(&sql).map_err(backend_err)?;
        let scored: Vec<(NodeId, Vec<u8>)> = stage2
            .query_map(params_from_iter(candidates.iter()), |r| {
                Ok((r.get::<_, i64>(0)? as NodeId, r.get::<_, Vec<u8>>(1)?))
            })
            .map_err(backend_err)?
            .collect::<Result<_, _>>()
            .map_err(backend_err)?;
        for (cand_id, cand_int8) in scored {
            let dot: f32 = own_int8
                .iter()
                .zip(cand_int8.iter())
                .map(|(&a, &b)| (a as i8 as f32) * (b as i8 as f32))
                .sum();
            let similarity = dot / INT8_UNIT_SCALE;
            if similarity >= threshold {
                let key = if id < cand_id {
                    (id, cand_id)
                } else {
                    (cand_id, id)
                };
                if seen.insert(key) {
                    pairs.push((key.0, key.1, similarity));
                }
            }
        }
    }
    Ok(pairs)
}

#[cfg(test)]
mod tests;
