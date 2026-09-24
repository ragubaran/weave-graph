use crate::error::StorageError;
use crate::model::{Edge, Node, NodeId};
use crate::notes::Note;
use crate::trace::TraceSpan;

pub const MAX_SEARCH_LIMIT: usize = 100;

/// Backend-agnostic persistence trait. No core logic
/// references a concrete backend — `weave-graph-store-sqlite` is the
/// default implementation; `weave-graph-store-turso` is an alternative
/// behind the same interface.
pub trait Storage {
    fn get_node(&self, id: NodeId) -> Result<Option<Node>, StorageError>;

    /// Outbound edges only (`source_id == node_id`).
    /// Bidirectional purge is in `purge_file_edges` below.
    fn get_edges(&self, node_id: NodeId) -> Result<Vec<Edge>, StorageError>;

    /// Inbound edges only (`target_id == node_id`). Used by `weave_trace_calls`
    /// for the "who calls this symbol" direction.
    fn get_callers(&self, node_id: NodeId) -> Result<Vec<Edge>, StorageError>;

    /// Insert or update by the node's natural key
    /// (`repo_id`, `path`, `symbol`, `line_start`). Returns the node's id.
    fn upsert_node(&mut self, node: &Node) -> Result<NodeId, StorageError>;

    /// Insert or update by the edge's natural key
    /// (`source_id`, `target_id`, `kind`). Returns the edge's id.
    fn upsert_edge(&mut self, edge: &Edge) -> Result<u32, StorageError>;

    /// Unweighted BFS over outbound edges. `Ok(Some(path))` includes both
    /// endpoints; `Ok(None)` means no path exists.
    fn query_path(&self, from: NodeId, to: NodeId) -> Result<Option<Vec<NodeId>>, StorageError>;

    fn schema_version(&self) -> Result<u32, StorageError>;

    /// Every node, ordered by id. The CSR graph is rebuilt from
    /// this on load — the SQL store is authoritative, the CSR a derived
    /// read structure with no sync path back.
    fn all_nodes(&self) -> Result<Vec<Node>, StorageError>;

    /// Every edge, ordered by `(source_id, target_id)`.
    fn all_edges(&self) -> Result<Vec<Edge>, StorageError>;

    /// Counts edges without forcing callers to retain the complete graph.
    fn edge_count(&self) -> Result<usize, StorageError> {
        Ok(self.all_edges()?.len())
    }

    /// Streams every node to `f` instead of materializing a `Vec<Node>`.
    /// Default forwards to `all_nodes` for backends that don't override it;
    /// `weave-graph-store-sqlite` overrides this to stream row-by-row —
    /// the `Vec<Node>` (five owned `String` fields each) `CsrGraph::load`
    /// used to require was the real RAM cost behind the measured Core
    /// Invariant 4 violation at 500k symbols, not the CSR's own layout.
    fn for_each_node(&self, f: &mut dyn FnMut(Node)) -> Result<(), StorageError> {
        for node in self.all_nodes()? {
            f(node);
        }
        Ok(())
    }

    /// Exact-match symbol lookup — the fast path every `weave_graph_core::
    /// resolve::resolve_symbol` caller should try before ever materializing
    /// `all_nodes()`. The default implementation streams via `for_each_node`
    /// so even a backend that hasn't overridden this never holds more than
    /// one `Node` at a time (PERF-G16: `all_nodes()`'s full `Vec<Node>` at
    /// 500k symbols, repeated per MCP request, was the measured RSS driver
    /// this exists to avoid). `weave-graph-store-sqlite` overrides it with
    /// a direct SQL lookup. Returns the first match on a natural-key
    /// collision, same as every existing exact-match resolver's contract.
    fn get_node_by_symbol(&self, symbol: &str) -> Result<Option<Node>, StorageError> {
        let mut found = None;
        self.for_each_node(&mut |node| {
            if found.is_none() && node.symbol == symbol {
                found = Some(node);
            }
        })?;
        Ok(found)
    }

    /// Streams every edge to `f` instead of materializing a `Vec<Edge>`.
    fn for_each_edge(&self, f: &mut dyn FnMut(Edge)) -> Result<(), StorageError> {
        for edge in self.all_edges()? {
            f(edge);
        }
        Ok(())
    }

    /// Purge all edges where source_id OR target_id belongs to the given file.
    /// Must be called before `purge_file_nodes` — deleting nodes first would
    /// violate the FK constraint and leave inbound edges from other files
    /// pointing at deleted node ids (Core Invariant 3).
    fn purge_file_edges(&mut self, repo_id: &str, path: &str) -> Result<u64, StorageError>;

    /// Purge all nodes for the given file. Call only after `purge_file_edges`.
    fn purge_file_nodes(&mut self, repo_id: &str, path: &str) -> Result<u64, StorageError>;

    /// Upserts unresolved references for a file.
    fn upsert_unresolved_refs(
        &mut self,
        repo_id: &str,
        path: &str,
        refs: &[String],
    ) -> Result<(), StorageError> {
        let _ = (repo_id, path, refs);
        Ok(())
    }

    /// Purges unresolved references for a file.
    fn purge_file_unresolved_refs(
        &mut self,
        repo_id: &str,
        path: &str,
    ) -> Result<u64, StorageError> {
        let _ = (repo_id, path);
        Ok(0)
    }

    /// Returns files that have an unresolved reference to the given short name.
    fn get_files_with_unresolved_refs(
        &self,
        repo_id: &str,
        short_name: &str,
    ) -> Result<Vec<String>, StorageError> {
        let _ = (repo_id, short_name);
        Ok(Vec::new())
    }

    /// Returns the unresolved short names referenced from one file — the
    /// inverse of [`Storage::get_files_with_unresolved_refs`], and the
    /// primitive `weave verify`'s phantom-symbol check reads per file.
    fn get_unresolved_refs_for_path(
        &self,
        repo_id: &str,
        path: &str,
    ) -> Result<Vec<String>, StorageError> {
        let _ = (repo_id, path);
        Ok(Vec::new())
    }

    /// Persist one pinned note; returns its id. Writes through
    /// `&self` — both backends' connections allow SQL writes on a shared
    /// reference, and the MCP pin tool only holds `&dyn Storage`.
    fn pin_note(&self, note: &Note) -> Result<i64, StorageError>;

    /// Every note row, including expired and orphaned ones — the reindex
    /// hook's input. Recall (`recall_notes`) is the filtered view.
    fn all_notes(&self) -> Result<Vec<Note>, StorageError>;

    /// The recall view: TTL filter applied at read time
    /// (`tier = 'crystallized' OR expires_at > now`) — no background
    /// sweep. Orphaned notes are included (reported, not dropped).
    fn recall_notes(&self, now: i64) -> Result<Vec<Note>, StorageError>;

    /// Moniker reattachment after a reindex: point the note at the
    /// symbol's new node id, or `None` to orphan it. `stale` replaces the
    /// stored staleness flag (recomputed from the content hash).
    fn reattach_note(
        &self,
        id: i64,
        target_node_id: Option<NodeId>,
        stale: bool,
    ) -> Result<(), StorageError>;

    /// Opportunistic cleanup of expired ephemeral notes — piggybacks on
    /// the reindex's own bulk-write transaction, never a separate pass.
    fn delete_expired_notes(&self, now: i64) -> Result<u64, StorageError>;

    /// Persist (or replace, by the `(trace_id, span_id)` natural key) one
    /// imported span. Defaulted to "unsupported" so minimal `Storage`
    /// implementations (test mocks, benches) need no stub rows; the SQLite
    /// and Turso backends override both.
    fn upsert_trace_span(&self, _span: &TraceSpan) -> Result<(), StorageError> {
        Err(StorageError::Backend(
            "this storage backend does not support trace spans".to_string(),
        ))
    }

    /// Every imported span, ordered by `(start_us, id)`.
    fn all_trace_spans(&self) -> Result<Vec<TraceSpan>, StorageError> {
        Ok(Vec::new())
    }

    /// Ranked node ids for a backend-specific full-text query, best match
    /// first (`weave search`). Defaulted to "unsupported" so backends
    /// without an FTS index (Turso today) need no stub — only
    /// `weave-graph-store-sqlite` (feature `fts`) overrides this, same
    /// shape as `upsert_trace_span` above.
    fn search_symbols(
        &self,
        _query: &str,
        _limit: usize,
        _visible: Option<&dyn Fn(&Node) -> bool>,
    ) -> Result<Vec<NodeId>, StorageError> {
        Err(StorageError::Backend(
            "this storage backend does not support symbol search".to_string(),
        ))
    }

    /// Every matching node for an FTS query, in deterministic
    /// `(path, line_start)` order — no ranking, no `LIMIT` — the
    /// exhaustive counterpart to `search_symbols`'s top-N BM25 ranking
    /// (`weave_find_all`, P10.4). Defaulted to "unsupported" for the same
    /// reason as `search_symbols`; only `weave-graph-store-sqlite`
    /// (feature `fts`) overrides this.
    fn find_all_symbols(&self, _pattern: &str) -> Result<Vec<Node>, StorageError> {
        Err(StorageError::Backend(
            "this storage backend does not support exhaustive symbol search".to_string(),
        ))
    }

    /// Three-stage ANN + rescore semantic search (`weave search
    /// --semantic`). `visible`, when given, must be applied to reranked
    /// candidates *before* the `limit` cap (Core Invariant 7, SEC-01) —
    /// never after, or a masked top hit starves a visible runner-up.
    /// Defaulted to "unsupported"; only `weave-graph-store-sqlite`
    /// (feature `vector`) overrides this.
    #[cfg(feature = "vector")]
    fn search_vector(
        &self,
        _embedder: &dyn crate::embedding::EmbeddingProvider,
        _query_text: &str,
        _limit: usize,
        _oversample: usize,
        _visible: Option<&dyn Fn(&Node) -> bool>,
    ) -> Result<Vec<NodeId>, StorageError> {
        Err(StorageError::Backend(
            "this storage backend does not support vector search".to_string(),
        ))
    }

    /// POL-02: self-KNN over every already-embedded chunk in `scope_ids`
    /// — no fresh text query, unlike [`Storage::search_vector`]. Finds
    /// `(node_a, node_b, approximate_cosine_similarity)` pairs already
    /// close in the existing vector index, deduped so `(a, b)`/`(b, a)`
    /// collapse to one entry. Advisory-only input for `weave policy
    /// drift`'s `semantic_coupling` rule — never `weave policy lint`'s
    /// hard-fail path. Defaulted to "unsupported"; only
    /// `weave-graph-store-sqlite` (feature `vector`) overrides this.
    #[cfg(feature = "vector")]
    fn find_similar_node_pairs(
        &self,
        _scope_ids: &[NodeId],
        _threshold: f32,
        _oversample: usize,
    ) -> Result<Vec<(NodeId, NodeId, f32)>, StorageError> {
        Err(StorageError::Backend(
            "this storage backend does not support semantic-coupling search".to_string(),
        ))
    }
}

/// Factory trait for creating storage backends from a file path.
/// Allows dependency injection and avoids hard-coding concrete backend types
/// in CLI/MCP layers (Core Invariant 4).
pub trait StorageBuilder {
    /// Open a read-write database at `path`, migrating if needed.
    fn open(&self, path: &std::path::Path) -> Result<Box<dyn Storage>, StorageError>;

    /// Open a fresh database at `rebuild_path` for bulk rebuilds.
    fn open_rebuild(&self, path: &std::path::Path) -> Result<Box<dyn Storage>, StorageError>;

    /// Open an in-memory database (no reload path).
    fn open_in_memory(&self) -> Result<Box<dyn Storage>, StorageError>;

    /// Open a read-only database at `path` (shared-snapshot mode for network mounts).
    fn open_read_only(&self, path: &std::path::Path) -> Result<Box<dyn Storage>, StorageError>;
}

#[cfg(test)]
mod tests;
