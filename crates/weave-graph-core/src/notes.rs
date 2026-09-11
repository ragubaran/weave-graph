//! Cross-agent memory graph (`impl.md` M2.10): notes pinned onto graph
//! nodes, persisted in plain SQL — never an LLM/embedding retrieval layer.
//! The data model is unconditional (the `Storage` trait and both backends
//! speak it); only hashing (below) needs the `notes` feature's `blake3`.

use crate::model::NodeId;

/// Lifecycle tier. Ephemeral notes carry a 24h TTL and vanish from recall
/// after it; crystallized notes never expire on their own and carry the
/// content-hash staleness signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteTier {
    Ephemeral,
    Crystallized,
}

impl NoteTier {
    pub fn as_str(self) -> &'static str {
        match self {
            NoteTier::Ephemeral => "ephemeral",
            NoteTier::Crystallized => "crystallized",
        }
    }

    /// Unknown tier strings (a future binary wrote them) read back as
    /// crystallized — the conservative choice: never silently expire data.
    pub fn parse(s: &str) -> NoteTier {
        if s == "ephemeral" {
            NoteTier::Ephemeral
        } else {
            NoteTier::Crystallized
        }
    }
}

/// One pinned note. `target_node_id` is `None` exactly when the note is
/// orphaned (its symbol's moniker no longer resolves after a reindex) —
/// orphaned notes are reported, never silently dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub id: i64,
    /// `None` once the note is orphaned by a reindex.
    pub target_node_id: Option<NodeId>,
    /// `path#qualified.symbol` — stable across purge-and-reinsert reindexes
    /// (Core Invariant 3), unlike the node id.
    pub moniker: String,
    pub kind: String,
    pub tier: NoteTier,
    pub author: String,
    pub content: String,
    /// Blake3 hex of the target symbol's source span at pin time
    /// (`None` when the source couldn't be read at pin time).
    pub content_hash: Option<String>,
    pub stale: bool,
    /// Unix seconds TTL for ephemeral notes; `None` for crystallized.
    pub expires_at: Option<i64>,
    pub created_at: i64,
}

/// Fixed 24h TTL for ephemeral notes (`impl.md` M2.10: a stated v1
/// simplicity choice — not user-configurable).
pub const EPHEMERAL_TTL_SECS: i64 = 24 * 60 * 60;

/// Blake3 hex digest of the target symbol's exact source span
/// (`line_start..=line_end`, 1-based, inclusive) — the staleness signal
/// recomputed at reindex time. Pure computation: callers read the file.
#[cfg(feature = "notes")]
pub fn hash_span(source: &str, line_start: u32, line_end: u32) -> String {
    let span = span_lines(source, line_start, line_end);
    blake3::hash(span.as_bytes()).to_hex().to_string()
}

#[cfg(feature = "notes")]
fn span_lines(source: &str, line_start: u32, line_end: u32) -> String {
    let start = (line_start as usize).saturating_sub(1);
    let end = (line_end as usize).min(source.lines().count());
    source
        .lines()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests;
