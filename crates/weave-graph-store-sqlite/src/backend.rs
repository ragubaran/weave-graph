use std::collections::{HashMap, VecDeque};
use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use weave_graph_core::{
    Edge, EdgeId, Node, NodeId, Note, NoteTier, Storage, StorageError, TraceSpan,
};

use crate::schema::{ensure_not_newer_than_supported, migrate, schema_version};

/// Default `Storage` implementation (`plan.md` §1.1), backed by `rusqlite`.
/// The SQL store is authoritative; nothing here depends on the CSR graph
/// built later in `weave-graph-core` from this data.
pub struct SqliteStorage {
    conn: Connection,
}

fn backend_err(e: rusqlite::Error) -> StorageError {
    StorageError::Backend(e.to_string())
}

impl SqliteStorage {
    /// Opens (creating if absent) the database at `path` and migrates it
    /// to the latest schema. WAL mode lets a reader hold the file open
    /// while `weave index` writes, matching the single-writer/read-heavy
    /// profile this crate is built for.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        #[cfg(feature = "vector")]
        crate::vector::ensure_vector_extension();
        let conn = Connection::open(path).map_err(backend_err)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(backend_err)?;
        migrate(&conn)?;
        #[cfg(feature = "fts")]
        crate::fts::ensure_fts_table(&conn)?;
        #[cfg(feature = "vector")]
        crate::vector::ensure_vector_table(&conn)?;
        Ok(Self { conn })
    }

    /// Opens an in-memory SQLite database migrated to latest schema.
    pub fn open_in_memory() -> Result<Self, StorageError> {
        #[cfg(feature = "vector")]
        crate::vector::ensure_vector_extension();
        let conn = Connection::open_in_memory().map_err(backend_err)?;
        migrate(&conn)?;
        #[cfg(feature = "fts")]
        crate::fts::ensure_fts_table(&conn)?;
        #[cfg(feature = "vector")]
        crate::vector::ensure_vector_table(&conn)?;
        Ok(Self { conn })
    }

    /// Opens a fresh database at `rebuild_path` for a bulk rebuild.
    /// Caller atomically renames it over the live path on success; a crash
    /// orphans the temp file and leaves the live index intact.
    pub fn open_rebuild(rebuild_path: &Path) -> Result<Self, StorageError> {
        #[cfg(feature = "vector")]
        crate::vector::ensure_vector_extension();
        let conn = Connection::open(rebuild_path).map_err(backend_err)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(backend_err)?;
        migrate(&conn)?;
        #[cfg(feature = "fts")]
        crate::fts::ensure_fts_table(&conn)?;
        #[cfg(feature = "vector")]
        crate::vector::ensure_vector_table(&conn)?;
        Ok(Self { conn })
    }

    /// Opens `path` read-only — the shared-snapshot mode (`plan.md` §1.4):
    /// callers use it on a network-mounted `.weave/`, so it never touches
    /// `journal_mode` (a read-only connection can't rewrite the file header
    /// anyway; the file is expected to already be non-WAL, via
    /// `export_read_only_snapshot`). Never migrates — a read-only connection
    /// can't write a schema upgrade — it only refuses a too-new schema, same
    /// rule `migrate` applies, just without the ability to fix an old one.
    pub fn open_read_only(path: &Path) -> Result<Self, StorageError> {
        #[cfg(feature = "vector")]
        crate::vector::ensure_vector_extension();
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(backend_err)?;
        ensure_not_newer_than_supported(&conn)?;
        Ok(Self { conn })
    }

    /// Writes a compacted copy of this database to `dest_path` in SQLite's
    /// default (non-WAL, `DELETE`) journal mode regardless of this
    /// connection's own mode — `VACUUM INTO` always starts the destination
    /// fresh, it never carries WAL over. This is the artifact
    /// `open_read_only` expects: safe to serve from a network filesystem
    /// without the `-wal`/`-shm` sidecar files WAL would need shared memory
    /// for (`AGENTS.md` §3, `plan.md` §1.4).
    pub fn export_read_only_snapshot(&self, dest_path: &Path) -> Result<(), StorageError> {
        self.conn
            .execute("VACUUM INTO ?1", params![dest_path.to_string_lossy()])
            .map_err(backend_err)?;
        Ok(())
    }

    /// Deletes every node of `kind` with zero inbound edges (impl.md M2.0's
    /// orphan-`doc_topic` sweep). Inbound-only: an outbound edge is not a
    /// reason to keep a derived node alive.
    pub fn purge_orphaned_nodes_by_kind(&self, kind: &str) -> Result<u64, StorageError> {
        let rows = self
            .conn
            .execute(
                "DELETE FROM nodes \
                 WHERE kind = ?1 AND id NOT IN (SELECT target_id FROM edges)",
                params![kind],
            )
            .map_err(backend_err)?;
        Ok(rows as u64)
    }

    /// Opens an explicit transaction around a bulk sequence of
    /// `upsert_node`/`upsert_edge` calls. SQLite autocommits every
    /// statement by default — one fsync per row is what capped M1.9's
    /// measured insert throughput at ~1,700-3,700 rows/sec and drove the
    /// Core Invariant 4 RAM/wall-clock violation at 500k symbols. Caller
    /// must pair this with `commit_bulk_write`; an error in between leaves
    /// the transaction open, rolled back automatically when `self` drops.
    pub fn begin_bulk_write(&self) -> Result<(), StorageError> {
        self.conn.execute_batch("BEGIN").map_err(backend_err)
    }

    /// Records (or replaces) one consumer's expectation of a provider's
    /// boundary contract hash, plus the sorted per-symbol entries that
    /// hash was computed from — the snapshot `check-contracts` diffs a
    /// later divergence against. The `contracts` table has no unique
    /// natural key, so replacement is an
    /// explicit delete-then-insert inside one transaction — an interrupted
    /// write can't leave two rows.
    pub fn upsert_contract(
        &mut self,
        service_a: &str,
        service_b: &str,
        contract_hash: &str,
        source_commit_sha: &str,
        entries_blob: &str,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction().map_err(backend_err)?;
        tx.execute(
            "DELETE FROM contracts WHERE service_a = ?1 AND service_b = ?2",
            params![service_a, service_b],
        )
        .map_err(backend_err)?;
        tx.execute(
            "INSERT INTO contracts (service_a, service_b, protocol, contract_hash, \
             source_commit_sha, published_at, entries_blob) VALUES (?1, ?2, 'source', ?3, ?4, \
             strftime('%s', 'now'), ?5)",
            params![
                service_a,
                service_b,
                contract_hash,
                source_commit_sha,
                entries_blob
            ],
        )
        .map_err(backend_err)?;
        tx.commit().map_err(backend_err)
    }

    /// Every contract expectation this repo (service_a) recorded, as
    /// `(provider label, expected hash, provider commit sha, entries blob)`
    /// tuples. `entries_blob` reads back as `""` for a row written before
    /// the `entries_blob` column existed, never a read error.
    pub fn contract_expectations(
        &self,
        service_a: &str,
    ) -> Result<Vec<(String, String, String, String)>, StorageError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT service_b, contract_hash, source_commit_sha, \
                 COALESCE(entries_blob, '') FROM contracts \
                 WHERE service_a = ?1 AND contract_hash IS NOT NULL",
            )
            .map_err(backend_err)?;
        let rows = stmt
            .query_map(params![service_a], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(backend_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(backend_err)
    }

    /// Commits a transaction opened by `begin_bulk_write`.
    pub fn commit_bulk_write(&self) -> Result<(), StorageError> {
        self.conn.execute_batch("COMMIT").map_err(backend_err)
    }

    /// Truncates the WAL file, flushing all pages to the main database file.
    /// This is strictly required before moving/renaming the database file
    /// via POSIX `rename(2)` so that the `-wal` and `-shm` files aren't left behind.
    pub fn checkpoint_wal(&self) -> Result<(), StorageError> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .map_err(backend_err)
    }

    /// Rebuilds the FTS5 symbol index from every current `nodes` row
    /// (`impl.md` M3.7 Tier 1) — call after writing nodes, inside the same
    /// bulk-write transaction.
    #[cfg(feature = "fts")]
    pub fn rebuild_fts_index(&self) -> Result<(), StorageError> {
        crate::fts::rebuild(&self.conn)
    }

    /// Ranked node ids for an FTS5 `MATCH` expression, best match first.
    #[cfg(feature = "fts")]
    pub fn search_symbols(
        &self,
        match_expr: &str,
        limit: usize,
    ) -> Result<Vec<NodeId>, StorageError> {
        crate::fts::search(&self.conn, match_expr, limit)
    }

    /// Rebuilds the `vec_chunks` semantic index (`impl.md` M3.7 Tier 2)
    /// from `chunks` — `(node_id, chunk_text)` pairs the caller already
    /// built from source file spans; this crate owns no file I/O.
    #[cfg(feature = "vector")]
    pub fn rebuild_vector_index(
        &self,
        embedder: &dyn weave_graph_core::embedding::EmbeddingProvider,
        chunks: &[(NodeId, String)],
    ) -> Result<(), StorageError> {
        crate::vector::rebuild(&self.conn, embedder, chunks)
    }

    /// Three-stage semantic search: binary ANN oversampled by
    /// `oversample`, reranked against int8 distance, capped at `limit`.
    /// `visible` (SEC-01), when given, is applied to the reranked
    /// candidates before the `limit` cap — never after — so a masked hit
    /// never displaces a visible one out of the returned set.
    #[cfg(feature = "vector")]
    pub fn search_vector(
        &self,
        embedder: &dyn weave_graph_core::embedding::EmbeddingProvider,
        query_text: &str,
        limit: usize,
        oversample: usize,
        visible: Option<&dyn Fn(&Node) -> bool>,
    ) -> Result<Vec<NodeId>, StorageError> {
        let node_visible = |id: NodeId| match self.get_node(id) {
            Ok(Some(node)) => visible.is_none_or(|v| v(&node)),
            _ => false,
        };
        let filter: Option<&dyn Fn(NodeId) -> bool> = if visible.is_some() {
            Some(&node_visible as &dyn Fn(NodeId) -> bool)
        } else {
            None
        };
        crate::vector::search(&self.conn, embedder, query_text, limit, oversample, filter)
    }

    /// Records (or replaces) one doc link, optionally carrying a
    /// provider-signed `Provenance` record (`impl.md` M2.3). The CLI
    /// never calls this with a record — a host application wires the
    /// real `ProvenanceProvider`; unsigned links stay renderable as
    /// plain edges without provenance.
    #[cfg(feature = "provenance")]
    pub fn upsert_doc_link(
        &mut self,
        doc_id: NodeId,
        section: &str,
        target_node_id: NodeId,
        kind: &str,
        provenance: Option<&weave_graph_core::provenance::Provenance>,
    ) -> Result<(), StorageError> {
        crate::doc_provenance::upsert_doc_link(
            &mut self.conn,
            doc_id,
            section,
            target_node_id,
            kind,
            provenance,
        )
    }

    /// Every doc link whose provenance columns are populated, in `id`
    /// order — the read side `weave report`/`weave export` render from.
    #[cfg(feature = "provenance")]
    pub fn doc_links_with_provenance(
        &self,
    ) -> Result<Vec<crate::doc_provenance::DocLinkProvenance>, StorageError> {
        crate::doc_provenance::doc_links_with_provenance(&self.conn)
    }
}

fn row_to_node(row: &rusqlite::Row) -> rusqlite::Result<Node> {
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
}

fn row_to_edge(row: &rusqlite::Row) -> rusqlite::Result<Edge> {
    Ok(Edge {
        id: row.get::<_, i64>(0)? as EdgeId,
        source_id: row.get::<_, i64>(1)? as NodeId,
        target_id: row.get::<_, i64>(2)? as NodeId,
        kind: row.get(3)?,
        weight: row.get(4)?,
    })
}

const NOTE_COLUMNS: &str = "id, target_node_id, moniker, kind, tier, author, content, \
     content_hash, stale, expires_at, created_at";

fn note_from_row(row: &rusqlite::Row) -> rusqlite::Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        target_node_id: row.get::<_, Option<i64>>(1)?.map(|v| v as NodeId),
        moniker: row.get(2)?,
        kind: row.get(3)?,
        tier: NoteTier::parse(&row.get::<_, String>(4)?),
        author: row.get(5)?,
        content: row.get(6)?,
        content_hash: row.get(7)?,
        stale: row.get::<_, i64>(8)? != 0,
        expires_at: row.get(9)?,
        created_at: row.get(10)?,
    })
}

const TRACE_SPAN_COLUMNS: &str = "trace_id, span_id, parent_span_id, service, name, symbol, \
     path, start_us, duration_us, status_code";

fn trace_span_from_row(row: &rusqlite::Row) -> rusqlite::Result<TraceSpan> {
    Ok(TraceSpan {
        trace_id: row.get(0)?,
        span_id: row.get(1)?,
        parent_span_id: row.get(2)?,
        service: row.get(3)?,
        name: row.get(4)?,
        symbol: row.get(5)?,
        path: row.get(6)?,
        start_us: row.get(7)?,
        duration_us: row.get(8)?,
        status_code: row.get(9)?,
    })
}

impl Storage for SqliteStorage {
    fn get_node(&self, id: NodeId) -> Result<Option<Node>, StorageError> {
        self.conn
            .query_row(
                "SELECT id, repo_id, path, symbol, kind, line_start, line_end, signature
                 FROM nodes WHERE id = ?1",
                params![id],
                row_to_node,
            )
            .optional()
            .map_err(backend_err)
    }

    fn get_edges(&self, node_id: NodeId) -> Result<Vec<Edge>, StorageError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, source_id, target_id, kind, weight FROM edges WHERE source_id = ?1",
            )
            .map_err(backend_err)?;
        let rows = stmt
            .query_map(params![node_id], row_to_edge)
            .map_err(backend_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend_err)
    }

    fn get_callers(&self, node_id: NodeId) -> Result<Vec<Edge>, StorageError> {
        // idx_edges_target (V2 migration) makes this O(k) not O(E).
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, source_id, target_id, kind, weight FROM edges WHERE target_id = ?1",
            )
            .map_err(backend_err)?;
        let rows = stmt
            .query_map(params![node_id], row_to_edge)
            .map_err(backend_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend_err)
    }

    fn all_nodes(&self) -> Result<Vec<Node>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, repo_id, path, symbol, kind, line_start, line_end, signature FROM nodes ORDER BY id")
            .map_err(backend_err)?;
        let rows = stmt.query_map([], row_to_node).map_err(backend_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend_err)
    }

    fn all_edges(&self) -> Result<Vec<Edge>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, source_id, target_id, kind, weight FROM edges ORDER BY source_id, target_id")
            .map_err(backend_err)?;
        let rows = stmt.query_map([], row_to_edge).map_err(backend_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend_err)
    }

    fn for_each_node(&self, f: &mut dyn FnMut(Node)) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, repo_id, path, symbol, kind, line_start, line_end, signature FROM nodes ORDER BY id")
            .map_err(backend_err)?;
        let mut rows = stmt.query([]).map_err(backend_err)?;
        while let Some(row) = rows.next().map_err(backend_err)? {
            f(row_to_node(row).map_err(backend_err)?);
        }
        Ok(())
    }

    fn for_each_edge(&self, f: &mut dyn FnMut(Edge)) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, source_id, target_id, kind, weight FROM edges ORDER BY source_id, target_id",
            )
            .map_err(backend_err)?;
        let mut rows = stmt.query([]).map_err(backend_err)?;
        while let Some(row) = rows.next().map_err(backend_err)? {
            f(row_to_edge(row).map_err(backend_err)?);
        }
        Ok(())
    }

    fn upsert_node(&mut self, node: &Node) -> Result<NodeId, StorageError> {
        self.conn
            .query_row(
                "INSERT INTO nodes (repo_id, path, symbol, kind, line_start, line_end, signature)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(repo_id, path, symbol, line_start) DO UPDATE SET
                    kind = excluded.kind,
                    line_end = excluded.line_end,
                    signature = excluded.signature
                 RETURNING id",
                params![
                    node.repo_id,
                    node.path,
                    node.symbol,
                    node.kind,
                    node.line_start,
                    node.line_end,
                    node.signature
                ],
                |row| row.get::<_, i64>(0),
            )
            .map(|id| id as NodeId)
            .map_err(backend_err)
    }

    fn upsert_edge(&mut self, edge: &Edge) -> Result<u32, StorageError> {
        self.conn
            .query_row(
                "INSERT INTO edges (source_id, target_id, kind, weight)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(source_id, target_id, kind) DO UPDATE SET
                    weight = excluded.weight
                 RETURNING id",
                params![edge.source_id, edge.target_id, edge.kind, edge.weight],
                |row| row.get::<_, i64>(0),
            )
            .map(|id| id as EdgeId)
            .map_err(backend_err)
    }

    fn query_path(&self, from: NodeId, to: NodeId) -> Result<Option<Vec<NodeId>>, StorageError> {
        if from == to {
            return Ok(Some(vec![from]));
        }
        let mut predecessor: HashMap<NodeId, NodeId> = HashMap::new();
        predecessor.insert(from, from);
        let mut queue = VecDeque::from([from]);

        while let Some(current) = queue.pop_front() {
            for edge in self.get_edges(current)? {
                if predecessor.contains_key(&edge.target_id) {
                    continue;
                }
                predecessor.insert(edge.target_id, current);
                if edge.target_id == to {
                    let mut path = vec![to];
                    let mut cur = current;
                    while cur != from {
                        path.push(cur);
                        cur = predecessor[&cur];
                    }
                    path.push(from);
                    path.reverse();
                    return Ok(Some(path));
                }
                queue.push_back(edge.target_id);
            }
        }
        Ok(None)
    }

    fn schema_version(&self) -> Result<u32, StorageError> {
        schema_version(&self.conn)
    }

    fn purge_file_edges(&mut self, repo_id: &str, path: &str) -> Result<u64, StorageError> {
        // Bidirectional purge required by plan.md §1.2a and Core Invariant 3.
        // Outbound-only delete leaves orphaned inbound edges from other files.
        let rows = self
            .conn
            .execute(
                "DELETE FROM edges
                  WHERE source_id IN (SELECT id FROM nodes WHERE repo_id = ?1 AND path = ?2)
                     OR target_id IN (SELECT id FROM nodes WHERE repo_id = ?1 AND path = ?2)",
                params![repo_id, path],
            )
            .map_err(backend_err)?;
        Ok(rows as u64)
    }

    fn purge_file_nodes(&mut self, repo_id: &str, path: &str) -> Result<u64, StorageError> {
        let rows = self
            .conn
            .execute(
                "DELETE FROM nodes WHERE repo_id = ?1 AND path = ?2",
                params![repo_id, path],
            )
            .map_err(backend_err)?;
        Ok(rows as u64)
    }

    fn pin_note(&self, note: &Note) -> Result<i64, StorageError> {
        self.conn
            .query_row(
                "INSERT INTO notes (target_node_id, moniker, kind, tier, author, content, \
                 content_hash, stale, expires_at, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 RETURNING id",
                params![
                    note.target_node_id.map(|v| v as i64),
                    note.moniker,
                    note.kind,
                    note.tier.as_str(),
                    note.author,
                    note.content,
                    note.content_hash,
                    note.stale as i64,
                    note.expires_at,
                    note.created_at,
                ],
                |row| row.get(0),
            )
            .map_err(backend_err)
    }

    fn all_notes(&self) -> Result<Vec<Note>, StorageError> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {NOTE_COLUMNS} FROM notes ORDER BY created_at, id"
            ))
            .map_err(backend_err)?;
        let rows = stmt.query_map([], note_from_row).map_err(backend_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend_err)
    }

    fn recall_notes(&self, now: i64) -> Result<Vec<Note>, StorageError> {
        // Read-time TTL filter, not a background sweep (impl.md M2.10).
        // Orphaned notes are included — reported, never silently dropped.
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {NOTE_COLUMNS} FROM notes
                 WHERE tier = 'crystallized' OR expires_at > ?1
                 ORDER BY created_at DESC, id"
            ))
            .map_err(backend_err)?;
        let rows = stmt
            .query_map(params![now], note_from_row)
            .map_err(backend_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend_err)
    }

    fn reattach_note(
        &self,
        id: i64,
        target_node_id: Option<NodeId>,
        stale: bool,
    ) -> Result<(), StorageError> {
        self.conn
            .execute(
                "UPDATE notes SET target_node_id = ?1, stale = ?2 WHERE id = ?3",
                params![target_node_id.map(|v| v as i64), stale as i64, id],
            )
            .map_err(backend_err)?;
        Ok(())
    }

    fn delete_expired_notes(&self, now: i64) -> Result<u64, StorageError> {
        let rows = self
            .conn
            .execute(
                "DELETE FROM notes WHERE tier = 'ephemeral' AND expires_at <= ?1",
                params![now],
            )
            .map_err(backend_err)?;
        Ok(rows as u64)
    }

    fn upsert_trace_span(&self, span: &TraceSpan) -> Result<(), StorageError> {
        self.conn
            .execute(
                "INSERT INTO trace_spans (trace_id, span_id, parent_span_id, service, name, \
                 symbol, path, start_us, duration_us, status_code)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(trace_id, span_id) DO UPDATE SET
                     parent_span_id = excluded.parent_span_id,
                     service = excluded.service,
                     name = excluded.name,
                     symbol = excluded.symbol,
                     path = excluded.path,
                     start_us = excluded.start_us,
                     duration_us = excluded.duration_us,
                     status_code = excluded.status_code",
                params![
                    span.trace_id,
                    span.span_id,
                    span.parent_span_id,
                    span.service,
                    span.name,
                    span.symbol,
                    span.path,
                    span.start_us,
                    span.duration_us,
                    span.status_code,
                ],
            )
            .map_err(backend_err)?;
        Ok(())
    }

    fn all_trace_spans(&self) -> Result<Vec<TraceSpan>, StorageError> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {TRACE_SPAN_COLUMNS} FROM trace_spans ORDER BY start_us, id"
            ))
            .map_err(backend_err)?;
        let rows = stmt
            .query_map([], trace_span_from_row)
            .map_err(backend_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend_err)
    }
}

#[cfg(test)]
mod tests;
