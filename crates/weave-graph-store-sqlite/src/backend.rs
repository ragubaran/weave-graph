use std::collections::{HashMap, VecDeque};
use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use weave_graph_core::{Edge, EdgeId, Node, NodeId, Storage, StorageError};

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
        let conn = Connection::open(path).map_err(backend_err)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(backend_err)?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Opens an in-memory SQLite database migrated to latest schema.
    pub fn open_in_memory() -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory().map_err(backend_err)?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Opens a fresh database at `rebuild_path` for a bulk rebuild.
    /// Caller atomically renames it over the live path on success; a crash
    /// orphans the temp file and leaves the live index intact.
    pub fn open_rebuild(rebuild_path: &Path) -> Result<Self, StorageError> {
        let conn = Connection::open(rebuild_path).map_err(backend_err)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(backend_err)?;
        migrate(&conn)?;
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
    /// boundary contract hash (`impl.md` M2.2). The `contracts` table has no
    /// unique natural key, so replacement is an explicit delete-then-insert
    /// inside one transaction — an interrupted write can't leave two rows.
    pub fn upsert_contract(
        &mut self,
        service_a: &str,
        service_b: &str,
        contract_hash: &str,
        source_commit_sha: &str,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction().map_err(backend_err)?;
        tx.execute(
            "DELETE FROM contracts WHERE service_a = ?1 AND service_b = ?2",
            params![service_a, service_b],
        )
        .map_err(backend_err)?;
        tx.execute(
            "INSERT INTO contracts (service_a, service_b, protocol, contract_hash, \
             source_commit_sha, published_at) VALUES (?1, ?2, 'source', ?3, ?4, \
             strftime('%s', 'now'))",
            params![service_a, service_b, contract_hash, source_commit_sha],
        )
        .map_err(backend_err)?;
        tx.commit().map_err(backend_err)
    }

    /// Every contract expectation this repo (service_a) recorded, as
    /// `(provider label, expected hash, provider commit sha)` triples.
    pub fn contract_expectations(
        &self,
        service_a: &str,
    ) -> Result<Vec<(String, String, String)>, StorageError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT service_b, contract_hash, source_commit_sha FROM contracts \
                 WHERE service_a = ?1 AND contract_hash IS NOT NULL",
            )
            .map_err(backend_err)?;
        let rows = stmt
            .query_map(params![service_a], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(backend_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(backend_err)
    }

    /// Commits a transaction opened by `begin_bulk_write`.
    pub fn commit_bulk_write(&self) -> Result<(), StorageError> {
        self.conn.execute_batch("COMMIT").map_err(backend_err)
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
}

#[cfg(test)]
mod tests;
