use rusqlite::Connection;
use weave_graph_core::StorageError;

/// Base schema (`plan.md` §1.1): `nodes`, `edges`, `doc_links`, `contracts`,
/// `schema_version`. Unique indices on each table's natural key make
/// `upsert_node`/`upsert_edge` idempotent under `INSERT ... ON CONFLICT`.
const V1_CREATE_TABLES: &str = "
CREATE TABLE nodes (
    id INTEGER PRIMARY KEY,
    repo_id TEXT NOT NULL,
    path TEXT NOT NULL,
    symbol TEXT NOT NULL,
    kind TEXT NOT NULL,
    line_start INTEGER NOT NULL,
    line_end INTEGER NOT NULL,
    signature TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_nodes_natural_key ON nodes(repo_id, path, symbol, line_start);

CREATE TABLE edges (
    id INTEGER PRIMARY KEY,
    source_id INTEGER NOT NULL REFERENCES nodes(id),
    target_id INTEGER NOT NULL REFERENCES nodes(id),
    kind TEXT NOT NULL,
    weight REAL NOT NULL DEFAULT 1.0
);
CREATE UNIQUE INDEX idx_edges_natural_key ON edges(source_id, target_id, kind);

CREATE TABLE doc_links (
    id INTEGER PRIMARY KEY,
    doc_id INTEGER NOT NULL,
    section TEXT NOT NULL,
    target_node_id INTEGER NOT NULL REFERENCES nodes(id),
    kind TEXT NOT NULL
);

CREATE TABLE contracts (
    id INTEGER PRIMARY KEY,
    service_a TEXT NOT NULL,
    service_b TEXT NOT NULL,
    protocol TEXT NOT NULL,
    schema_ref TEXT,
    contract_hash TEXT,
    source_commit_sha TEXT,
    published_at INTEGER
);

CREATE TABLE schema_version (
    version INTEGER PRIMARY KEY,
    applied_at INTEGER NOT NULL
);
";

/// M1.4's per-file purge (`DELETE FROM edges WHERE source_id IN (SELECT id
/// FROM nodes WHERE path = ?) OR target_id IN (...)`) and traversal both
/// need these — added as a real migration (not folded into v1) so the
/// upgrade-an-existing-db path is exercised now while it's cheap.
const V2_TRAVERSAL_INDICES: &str = "
CREATE INDEX idx_edges_source ON edges(source_id);
CREATE INDEX idx_edges_target ON edges(target_id);
CREATE INDEX idx_nodes_repo_path ON nodes(repo_id, path);
";

/// M2.3 (`provenance` feature): nullable provider-signature columns on
/// `doc_links`. Unconditional — schema version must not depend on Cargo
/// features, and M1.1 already set the pre-add-columns precedent. The
/// commit column exists because the Merkle root is a one-way hash: the
/// record must round-trip `(doc_id, commit_hash, root, signature)` in
/// full for `verify` to recompute the root after a store read.
const V3_DOC_LINK_PROVENANCE: &str = "
ALTER TABLE doc_links ADD COLUMN provenance_commit TEXT;
ALTER TABLE doc_links ADD COLUMN provenance_hash TEXT;
ALTER TABLE doc_links ADD COLUMN provenance_signature TEXT;
";

const MIGRATIONS: &[(u32, &str)] = &[
    (1, V1_CREATE_TABLES),
    (2, V2_TRAVERSAL_INDICES),
    (3, V3_DOC_LINK_PROVENANCE),
];

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
    for (version, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > current) {
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
