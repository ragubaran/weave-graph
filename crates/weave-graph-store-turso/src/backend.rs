use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::path::Path;

use weave_graph_core::{
    Edge, EdgeId, Node, NodeId, Note, NoteTier, Storage, StorageError, TraceSpan,
};

use crate::schema::{migrate, schema_version};

const NOTE_COLUMNS: &str = "id, target_node_id, moniker, kind, tier, author, content, \
     content_hash, stale, expires_at, created_at";

fn note_from_row(row: &libsql::Row) -> libsql::Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        target_node_id: row.get::<Option<i64>>(1)?.map(|v| v as NodeId),
        moniker: row.get(2)?,
        kind: row.get(3)?,
        tier: NoteTier::parse(&row.get::<String>(4)?),
        author: row.get(5)?,
        content: row.get(6)?,
        content_hash: row.get(7)?,
        stale: row.get::<i64>(8)? != 0,
        expires_at: row.get(9)?,
        created_at: row.get(10)?,
    })
}

const TRACE_SPAN_COLUMNS: &str = "trace_id, span_id, parent_span_id, service, name, symbol, \
     path, start_us, duration_us, status_code";

fn trace_span_from_row(row: &libsql::Row) -> libsql::Result<TraceSpan> {
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

/// libSQL-backed `Storage` implementation (`impl.md` M2.7), embedded
/// (`Builder::new_local` — no network, no server). Same schema and
/// migrations as `weave-graph-store-sqlite`, replayed verbatim.
pub struct TursoStorage {
    conn: libsql::Connection,
}

fn backend_err(e: libsql::Error) -> StorageError {
    StorageError::Backend(e.to_string())
}

/// libSQL's embedded engine runs queries inline on the calling thread (no
/// task spawning under the `core` feature), so a plain executor park is a
/// lossless bridge to the synchronous `Storage` trait — no runtime needed.
fn block_on<T>(fut: impl Future<Output = libsql::Result<T>>) -> Result<T, StorageError> {
    futures::executor::block_on(fut).map_err(backend_err)
}

fn row_to_node(row: &libsql::Row) -> libsql::Result<Node> {
    Ok(Node {
        id: row.get::<i64>(0)? as NodeId,
        repo_id: row.get(1)?,
        path: row.get(2)?,
        symbol: row.get(3)?,
        kind: row.get(4)?,
        line_start: row.get::<i64>(5)? as u32,
        line_end: row.get::<i64>(6)? as u32,
        signature: row.get(7)?,
    })
}

fn row_to_edge(row: &libsql::Row) -> libsql::Result<Edge> {
    Ok(Edge {
        id: row.get::<i64>(0)? as EdgeId,
        source_id: row.get::<i64>(1)? as NodeId,
        target_id: row.get::<i64>(2)? as NodeId,
        kind: row.get(3)?,
        weight: row.get(4)?,
    })
}

impl TursoStorage {
    /// Opens (creating if absent) the database at `path` and migrates it
    /// to the latest schema — mirrors `SqliteStorage::open`.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        let db = block_on(libsql::Builder::new_local(path).build())?;
        Self::connect(db)
    }

    /// Opens an in-memory libSQL database migrated to latest schema.
    pub fn open_in_memory() -> Result<Self, StorageError> {
        let db = block_on(libsql::Builder::new_local(":memory:").build())?;
        Self::connect(db)
    }

    fn connect(db: libsql::Database) -> Result<Self, StorageError> {
        let conn = db.connect().map_err(backend_err)?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Opens an explicit transaction around a bulk sequence of
    /// `upsert_node`/`upsert_edge` calls — same autocommit-per-statement
    /// hazard as the rusqlite backend (M1.9's measured bottleneck).
    /// Caller must pair with `commit_bulk_write`.
    pub fn begin_bulk_write(&self) -> Result<(), StorageError> {
        block_on(self.conn.execute_batch("BEGIN"))?;
        Ok(())
    }

    /// Commits a transaction opened by `begin_bulk_write`.
    pub fn commit_bulk_write(&self) -> Result<(), StorageError> {
        block_on(self.conn.execute_batch("COMMIT"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;

impl Storage for TursoStorage {
    fn get_node(&self, id: NodeId) -> Result<Option<Node>, StorageError> {
        block_on(async {
            let mut rows = self
                .conn
                .query(
                    "SELECT id, repo_id, path, symbol, kind, line_start, line_end, signature
                     FROM nodes WHERE id = ?1",
                    libsql::params![id],
                )
                .await?;
            match rows.next().await? {
                Some(row) => Ok(Some(row_to_node(&row)?)),
                None => Ok(None),
            }
        })
    }

    fn get_edges(&self, node_id: NodeId) -> Result<Vec<Edge>, StorageError> {
        block_on(async {
            let mut rows = self
                .conn
                .query(
                    "SELECT id, source_id, target_id, kind, weight FROM edges WHERE source_id = ?1",
                    libsql::params![node_id],
                )
                .await?;
            let mut edges = Vec::new();
            while let Some(row) = rows.next().await? {
                edges.push(row_to_edge(&row)?);
            }
            Ok(edges)
        })
    }

    fn get_callers(&self, node_id: NodeId) -> Result<Vec<Edge>, StorageError> {
        block_on(async {
            let mut rows = self
                .conn
                .query(
                    "SELECT id, source_id, target_id, kind, weight FROM edges WHERE target_id = ?1",
                    libsql::params![node_id],
                )
                .await?;
            let mut edges = Vec::new();
            while let Some(row) = rows.next().await? {
                edges.push(row_to_edge(&row)?);
            }
            Ok(edges)
        })
    }

    fn all_nodes(&self) -> Result<Vec<Node>, StorageError> {
        block_on(async {
            let mut rows = self
                .conn
                .query(
                    "SELECT id, repo_id, path, symbol, kind, line_start, line_end, signature FROM nodes ORDER BY id",
                    (),
                )
                .await?;
            let mut nodes = Vec::new();
            while let Some(row) = rows.next().await? {
                nodes.push(row_to_node(&row)?);
            }
            Ok(nodes)
        })
    }

    fn all_edges(&self) -> Result<Vec<Edge>, StorageError> {
        block_on(async {
            let mut rows = self
                .conn
                .query(
                    "SELECT id, source_id, target_id, kind, weight FROM edges ORDER BY source_id, target_id",
                    (),
                )
                .await?;
            let mut edges = Vec::new();
            while let Some(row) = rows.next().await? {
                edges.push(row_to_edge(&row)?);
            }
            Ok(edges)
        })
    }

    fn for_each_node(&self, f: &mut dyn FnMut(Node)) -> Result<(), StorageError> {
        block_on(async {
            let mut rows = self
                .conn
                .query(
                    "SELECT id, repo_id, path, symbol, kind, line_start, line_end, signature FROM nodes ORDER BY id",
                    (),
                )
                .await?;
            while let Some(row) = rows.next().await? {
                f(row_to_node(&row)?);
            }
            Ok(())
        })
    }

    fn for_each_edge(&self, f: &mut dyn FnMut(Edge)) -> Result<(), StorageError> {
        block_on(async {
            let mut rows = self
                .conn
                .query(
                    "SELECT id, source_id, target_id, kind, weight FROM edges ORDER BY source_id, target_id",
                    (),
                )
                .await?;
            while let Some(row) = rows.next().await? {
                f(row_to_edge(&row)?);
            }
            Ok(())
        })
    }

    fn upsert_node(&mut self, node: &Node) -> Result<NodeId, StorageError> {
        block_on(async {
            let mut rows = self
                .conn
                .query(
                    "INSERT INTO nodes (repo_id, path, symbol, kind, line_start, line_end, signature)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(repo_id, path, symbol, line_start) DO UPDATE SET
                        kind = excluded.kind,
                        line_end = excluded.line_end,
                        signature = excluded.signature
                     RETURNING id",
                    (
                        node.repo_id.as_str(),
                        node.path.as_str(),
                        node.symbol.as_str(),
                        node.kind.as_str(),
                        node.line_start,
                        node.line_end,
                        node.signature.as_str(),
                    ),
                )
                .await?;
            match rows.next().await? {
                Some(row) => Ok(row.get::<i64>(0)? as NodeId),
                None => Err(libsql::Error::QueryReturnedNoRows),
            }
        })
    }

    fn upsert_edge(&mut self, edge: &Edge) -> Result<u32, StorageError> {
        block_on(async {
            let mut rows = self
                .conn
                .query(
                    "INSERT INTO edges (source_id, target_id, kind, weight)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(source_id, target_id, kind) DO UPDATE SET
                        weight = excluded.weight
                     RETURNING id",
                    (
                        edge.source_id,
                        edge.target_id,
                        edge.kind.as_str(),
                        edge.weight,
                    ),
                )
                .await?;
            match rows.next().await? {
                Some(row) => Ok(row.get::<i64>(0)? as EdgeId),
                None => Err(libsql::Error::QueryReturnedNoRows),
            }
        })
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
        block_on(async {
            let rows = self
                .conn
                .execute(
                    "DELETE FROM edges
                      WHERE source_id IN (SELECT id FROM nodes WHERE repo_id = ?1 AND path = ?2)
                         OR target_id IN (SELECT id FROM nodes WHERE repo_id = ?1 AND path = ?2)",
                    (repo_id, path),
                )
                .await?;
            Ok(rows)
        })
    }

    fn purge_file_nodes(&mut self, repo_id: &str, path: &str) -> Result<u64, StorageError> {
        block_on(async {
            let rows = self
                .conn
                .execute(
                    "DELETE FROM nodes WHERE repo_id = ?1 AND path = ?2",
                    (repo_id, path),
                )
                .await?;
            Ok(rows)
        })
    }

    fn pin_note(&self, note: &Note) -> Result<i64, StorageError> {
        block_on(async {
            let mut rows = self
                .conn
                .query(
                    "INSERT INTO notes (target_node_id, moniker, kind, tier, author, content, \
                     content_hash, stale, expires_at, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                     RETURNING id",
                    (
                        note.target_node_id.map(|v| v as i64),
                        note.moniker.as_str(),
                        note.kind.as_str(),
                        note.tier.as_str(),
                        note.author.as_str(),
                        note.content.as_str(),
                        note.content_hash.clone(),
                        note.stale,
                        note.expires_at,
                        note.created_at,
                    ),
                )
                .await?;
            match rows.next().await? {
                Some(row) => Ok(row.get::<i64>(0)?),
                None => Err(libsql::Error::QueryReturnedNoRows),
            }
        })
    }

    fn all_notes(&self) -> Result<Vec<Note>, StorageError> {
        block_on(async {
            let mut rows = self
                .conn
                .query(
                    &format!("SELECT {NOTE_COLUMNS} FROM notes ORDER BY created_at, id"),
                    (),
                )
                .await?;
            let mut notes = Vec::new();
            while let Some(row) = rows.next().await? {
                notes.push(note_from_row(&row)?);
            }
            Ok(notes)
        })
    }

    fn recall_notes(&self, now: i64) -> Result<Vec<Note>, StorageError> {
        // Read-time TTL filter, not a background sweep (impl.md M2.10).
        block_on(async {
            let mut rows = self
                .conn
                .query(
                    &format!(
                        "SELECT {NOTE_COLUMNS} FROM notes
                         WHERE tier = 'crystallized' OR expires_at > ?1
                         ORDER BY created_at DESC, id"
                    ),
                    libsql::params![now],
                )
                .await?;
            let mut notes = Vec::new();
            while let Some(row) = rows.next().await? {
                notes.push(note_from_row(&row)?);
            }
            Ok(notes)
        })
    }

    fn reattach_note(
        &self,
        id: i64,
        target_node_id: Option<NodeId>,
        stale: bool,
    ) -> Result<(), StorageError> {
        block_on(async {
            self.conn
                .execute(
                    "UPDATE notes SET target_node_id = ?1, stale = ?2 WHERE id = ?3",
                    (target_node_id.map(|v| v as i64), stale, id),
                )
                .await?;
            Ok(())
        })
    }

    fn delete_expired_notes(&self, now: i64) -> Result<u64, StorageError> {
        block_on(async {
            let rows = self
                .conn
                .execute(
                    "DELETE FROM notes WHERE tier = 'ephemeral' AND expires_at <= ?1",
                    libsql::params![now],
                )
                .await?;
            Ok(rows)
        })
    }

    fn upsert_trace_span(&self, span: &TraceSpan) -> Result<(), StorageError> {
        block_on(async {
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
                    libsql::params![
                        span.trace_id.as_str(),
                        span.span_id.as_str(),
                        span.parent_span_id.clone(),
                        span.service.clone(),
                        span.name.as_str(),
                        span.symbol.clone(),
                        span.path.clone(),
                        span.start_us,
                        span.duration_us,
                        span.status_code.as_str(),
                    ],
                )
                .await?;
            Ok(())
        })
    }

    fn all_trace_spans(&self) -> Result<Vec<TraceSpan>, StorageError> {
        block_on(async {
            let mut rows = self
                .conn
                .query(
                    &format!("SELECT {TRACE_SPAN_COLUMNS} FROM trace_spans ORDER BY start_us, id"),
                    (),
                )
                .await?;
            let mut spans = Vec::new();
            while let Some(row) = rows.next().await? {
                spans.push(trace_span_from_row(&row)?);
            }
            Ok(spans)
        })
    }
}
