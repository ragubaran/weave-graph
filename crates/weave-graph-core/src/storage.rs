use crate::error::StorageError;
use crate::model::{Edge, Node, NodeId};
use crate::notes::Note;

/// Backend-agnostic persistence trait (`plan.md` §0.4, §1.1). No core logic
/// references a concrete backend — `weave-graph-store-sqlite` is the
/// default implementation; `weave-graph-store-turso` is an alternative
/// behind the same interface.
pub trait Storage {
    fn get_node(&self, id: NodeId) -> Result<Option<Node>, StorageError>;

    /// Outbound edges only (`source_id == node_id`).
    /// Bidirectional purge is in `purge_file_edges` below.
    fn get_edges(&self, node_id: NodeId) -> Result<Vec<Edge>, StorageError>;

    /// Inbound edges only (`target_id == node_id`). Used by `weave_trace_calls`
    /// for the "who calls this symbol" direction (`plan.md` §1.5).
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

    /// Every node, ordered by id. The CSR graph (M1.3) is rebuilt from
    /// this on load — the SQL store is authoritative, the CSR a derived
    /// read structure with no sync path back.
    fn all_nodes(&self) -> Result<Vec<Node>, StorageError>;

    /// Every edge, ordered by `(source_id, target_id)`.
    fn all_edges(&self) -> Result<Vec<Edge>, StorageError>;

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
    /// pointing at deleted node ids (`plan.md` §1.2a, Core Invariant 3).
    fn purge_file_edges(&mut self, repo_id: &str, path: &str) -> Result<u64, StorageError>;

    /// Purge all nodes for the given file. Call only after `purge_file_edges`.
    fn purge_file_nodes(&mut self, repo_id: &str, path: &str) -> Result<u64, StorageError>;

    /// Persist one pinned note (M2.10); returns its id. Writes through
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
}

#[cfg(test)]
mod tests;
