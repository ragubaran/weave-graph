/// 32-bit interned node id. Paths and symbols are mapped to `uint32` so
/// the CSR adjacency stays integer-compacted.
pub type NodeId = u32;

/// 32-bit interned edge id.
pub type EdgeId = u32;

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: NodeId,
    pub repo_id: String,
    pub path: String,
    pub symbol: String,
    pub kind: String,
    pub line_start: u32,
    pub line_end: u32,
    pub signature: String,
}

/// `kind` stays a free-form string, never a closed enum — new relationship
/// types (`CALLS_EXACT`, `CALLS_DYNAMIC`, `IMPORTS`, ...) are additive data,
/// not a migration plus recompile. `extractor`/`resolution_kind` (P10.5)
/// follow the same "free-form string, additive" philosophy: `extractor`
/// names which language backend produced the edge (e.g. `"rust"`), and
/// `resolution_kind` names how its endpoints were resolved (e.g.
/// `"SAME_FILE_EXACT"`, see [`edge_confidence`]). Both are `None` for an
/// edge written before this field existed or by a caller that never sets
/// them — never a schema requirement, only richer provenance when present.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub id: EdgeId,
    pub source_id: NodeId,
    pub target_id: NodeId,
    pub kind: String,
    pub weight: f64,
    pub extractor: Option<String>,
    pub resolution_kind: Option<String>,
}

/// A coarse, typed confidence class (P10.5) — never an uncalibrated float
/// score. Two values only: this resolver either landed on exactly one
/// candidate, or it didn't and every same-named candidate became its own
/// edge (`CALLS_DYNAMIC`'s heuristic fan-out). A third "somewhat sure"
/// tier isn't something this resolver can actually produce, so it isn't
/// modeled — inventing one would misrepresent what the data supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    Exact,
    Heuristic,
}

/// Classifies an edge's confidence. `resolution_kind` (when present) is
/// authoritative; an edge written before P10.5 (`resolution_kind: None`)
/// falls back to the coarser two-tier `kind` taxonomy that already
/// existed (`CALLS_DYNAMIC` is always heuristic fan-out; every other kind,
/// including the structural ones, is exact) — so old data still classifies
/// sensibly instead of reading as "unknown".
pub fn edge_confidence(kind: &str, resolution_kind: Option<&str>) -> Confidence {
    match resolution_kind {
        Some(AMBIGUOUS_HEURISTIC) => Confidence::Heuristic,
        Some(_) => Confidence::Exact,
        None if kind == "CALLS_DYNAMIC" => Confidence::Heuristic,
        None => Confidence::Exact,
    }
}

/// The one `resolution_kind` value [`edge_confidence`] treats as
/// heuristic — shared with `weave-graph-parse::resolve`, which is the
/// only writer of this column, so the string never drifts out of sync
/// between the two crates.
pub const AMBIGUOUS_HEURISTIC: &str = "AMBIGUOUS_HEURISTIC";

#[cfg(test)]
mod tests;
