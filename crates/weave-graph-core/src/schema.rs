//! The schema every `Storage` backend must produce, expressed as
//! backend-agnostic SQL constants. Lives in core because
//! every store crate shares it — and because `rusqlite`'s bundled
//! `libsqlite3-sys` and `libsql-ffi` both statically define the SQLite C
//! symbols, the store crates can never be linked into one binary, so the
//! SQL cannot live in either of them.

/// Highest schema version any migration in `MIGRATIONS` brings a database
/// to — round-trip tests assert against it.
pub const LATEST_SCHEMA_VERSION: u32 = 9;

/// Base schema: `nodes`, `edges`, `doc_links`, `contracts`,
/// `schema_version`. Unique indices on each table's natural key make
/// `upsert_node`/`upsert_edge` idempotent under `INSERT ... ON CONFLICT`.
pub const V1_CREATE_TABLES: &str = "
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

/// Per-file purge (`DELETE FROM edges WHERE source_id IN (SELECT id
/// FROM nodes WHERE path = ?) OR target_id IN (...)`) and traversal both
/// need these — added as a real migration (not folded into v1) so the
/// upgrade-an-existing-db path is exercised now while it's cheap.
pub const V2_TRAVERSAL_INDICES: &str = "
CREATE INDEX idx_edges_source ON edges(source_id);
CREATE INDEX idx_edges_target ON edges(target_id);
CREATE INDEX idx_nodes_repo_path ON nodes(repo_id, path);
";

/// `provenance` feature: nullable provider-signature columns on
/// `doc_links`. Unconditional — schema version must not depend on Cargo
/// features, and every ALTER TABLE migration here follows the same
/// add-columns-unconditionally precedent. The commit column exists because
/// the Merkle root is a one-way hash: the record must round-trip `(doc_id,
/// commit_hash, root, signature)` in full for `verify` to recompute the
/// root after a store read.
pub const V3_DOC_LINK_PROVENANCE: &str = "
ALTER TABLE doc_links ADD COLUMN provenance_commit TEXT;
ALTER TABLE doc_links ADD COLUMN provenance_hash TEXT;
ALTER TABLE doc_links ADD COLUMN provenance_signature TEXT;
";

/// `notes` feature: cross-agent memory graph. The *table* lands
/// unconditionally — same reasoning as V3's own precedent: schema version
/// must not depend on Cargo features, and a default binary must be able to
/// open a notes-enabled repo's database without a SchemaTooNew refusal.
/// Everything that writes/reads it (pin/list/recall, staleness, expiry) is
/// feature-gated in the CLI and MCP crates.
pub const V4_NOTES_TABLE: &str = "
CREATE TABLE notes (
    id INTEGER PRIMARY KEY,
    target_node_id INTEGER REFERENCES nodes(id) ON DELETE SET NULL,
    moniker TEXT NOT NULL,
    kind TEXT NOT NULL,
    tier TEXT NOT NULL,
    author TEXT NOT NULL,
    content TEXT NOT NULL,
    content_hash TEXT,
    stale INTEGER NOT NULL DEFAULT 0,
    expires_at INTEGER,
    created_at INTEGER NOT NULL
);
CREATE INDEX idx_notes_moniker ON notes(moniker);
";

/// `otel` feature: imported distributed trace spans, overlaid onto
/// graph nodes by symbol at *query* time — no node-id FK, deliberately:
/// a reindex renumbers ids, and re-resolution by symbol is what keeps a
/// span from dangling (Core Invariant 3). The table lands unconditionally
/// (V4's precedent); only the CLI importer/query is feature-gated.
pub const V5_TRACE_SPANS_TABLE: &str = "
CREATE TABLE trace_spans (
    id INTEGER PRIMARY KEY,
    trace_id TEXT NOT NULL,
    span_id TEXT NOT NULL,
    parent_span_id TEXT,
    service TEXT,
    name TEXT NOT NULL,
    symbol TEXT,
    path TEXT,
    start_us INTEGER NOT NULL,
    duration_us INTEGER NOT NULL,
    status_code TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_trace_spans_natural_key ON trace_spans(trace_id, span_id);
";

/// The sorted per-symbol entries a contract hash was computed from, so a
/// later divergence can be diffed symbol-by-symbol instead of only
/// reporting "hashes differ". Nullable — a row written before this
/// migration reads back as `COALESCE(entries_blob, '')`, an empty map,
/// never a read error.
pub const V6_CONTRACT_ENTRIES: &str = "
ALTER TABLE contracts ADD COLUMN entries_blob TEXT;
";

pub const V7_RESOLVER_INPUTS: &str = "
CREATE TABLE unresolved_refs (
    repo_id TEXT NOT NULL,
    path TEXT NOT NULL,
    short_name TEXT NOT NULL
);
CREATE INDEX idx_unresolved_refs_name ON unresolved_refs(short_name);
CREATE INDEX idx_unresolved_refs_path ON unresolved_refs(repo_id, path);
";

/// The composite index persists overload-safe identity across span changes.
/// The nullable compatibility column is not populated because duplicating
/// the indexed identity text increases database and WAL size without value.
pub const V8_NODE_SEMANTIC_KEY: &str = "
ALTER TABLE nodes ADD COLUMN semantic_key TEXT;
CREATE INDEX idx_nodes_semantic_key ON nodes(repo_id, path, symbol, kind, signature);
";

/// The natural-key index already covers the production identity lookup prefix.
/// Keeping this wider duplicate index raised write RSS without query benefit.
pub const V9_DROP_UNUSED_SEMANTIC_KEY_INDEX: &str = "
DROP INDEX IF EXISTS idx_nodes_semantic_key;
";

/// Versioned migration history shared by every storage backend.
pub const MIGRATIONS: &[(u32, &str)] = &[
    (1, V1_CREATE_TABLES),
    (2, V2_TRAVERSAL_INDICES),
    (3, V3_DOC_LINK_PROVENANCE),
    (4, V4_NOTES_TABLE),
    (5, V5_TRACE_SPANS_TABLE),
    (6, V6_CONTRACT_ENTRIES),
    (7, V7_RESOLVER_INPUTS),
    (8, V8_NODE_SEMANTIC_KEY),
    (9, V9_DROP_UNUSED_SEMANTIC_KEY_INDEX),
];

/// Returns unapplied migrations in version order, independent of declaration order.
pub fn migrations_after(current: u32) -> Vec<(u32, &'static str)> {
    migrations_after_from(MIGRATIONS, current)
}

fn migrations_after_from(
    migrations: &[(u32, &'static str)],
    current: u32,
) -> Vec<(u32, &'static str)> {
    let mut pending = migrations
        .iter()
        .copied()
        .filter(|(version, _)| *version > current)
        .collect::<Vec<_>>();
    pending.sort_unstable_by_key(|(version, _)| *version);
    pending
}

#[cfg(test)]
mod tests {
    use super::migrations_after_from;

    #[test]
    fn pending_migrations_are_sorted_by_version_not_declaration_order() {
        let migrations = [(3, "third"), (1, "first"), (2, "second")];
        let versions = migrations_after_from(&migrations, 0)
            .into_iter()
            .map(|(version, _)| version)
            .collect::<Vec<_>>();
        assert_eq!(versions, [1, 2, 3]);
    }
}
