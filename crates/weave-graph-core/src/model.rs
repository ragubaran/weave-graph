/// 32-bit interned node id. `plan.md` §1.1: paths and symbols are mapped to
/// `uint32` so the CSR adjacency built in M1.3 stays integer-compacted.
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
/// not a migration plus recompile (`plan.md` §1.1).
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub id: EdgeId,
    pub source_id: NodeId,
    pub target_id: NodeId,
    pub kind: String,
    pub weight: f64,
}

#[cfg(test)]
mod tests;
