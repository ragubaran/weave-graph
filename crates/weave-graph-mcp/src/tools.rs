use weave_graph_core::resolve::resolve_symbol as core_resolve_symbol;
use weave_graph_core::{Node, NodeId};

/// Arguments for `weave_repo_map`. Caps output to stay within ~200 tokens.
pub struct RepoMapArgs {
    /// Max number of files to surface (default: 50). Ignored when
    /// `max_tokens` is set — token-estimate truncation takes over.
    pub max_files: usize,
    /// `Some(true)` = module-level orientation (one line per Louvain
    /// module: label, file count, symbol count, cross-edges, member
    /// files); `None`/`Some(false)` = the file-level default. Opt-in:
    /// the default stays byte-identical
    /// until module coverage is proven in real agent use.
    pub module: Option<bool>,
    /// Token-estimate ceiling: when set, `max_files` truncation
    /// is replaced by shedding lines until the output fits. Omitting it
    /// keeps today's exact behavior.
    pub max_tokens: Option<usize>,
}

impl Default for RepoMapArgs {
    fn default() -> Self {
        Self {
            max_files: 50,
            module: None,
            max_tokens: None,
        }
    }
}

/// Arguments for `weave_file_api`.
pub struct FileApiArgs<'a> {
    pub paths: &'a [&'a str],
    /// Token-estimate ceiling: sheds detail in tiers (full wiring
    /// cards → per-file symbol names → per-file counts) instead of
    /// returning an unbounded blob. `None` = today's behavior.
    pub max_tokens: Option<usize>,
}

/// Arguments for `weave_trace_calls`.
pub struct TraceCallsArgs<'a> {
    pub symbol: &'a str,
    /// Max hop depth for both incoming and outgoing traversal.
    pub depth: u32,
    /// Token-estimate ceiling: truncates the chains with explicit
    /// "... and N more" markers when the full trace would exceed it.
    pub max_tokens: Option<usize>,
    /// P10.5: drops heuristically-resolved callers from the *incoming*
    /// chain (`weave_graph_core::edge_confidence`) — the outgoing chain
    /// walks the CSR, which carries no per-edge kind, so this can't
    /// apply there; `false` is byte-identical to today's behavior.
    pub precise_only: bool,
}

/// Arguments for `weave_impact_radius`.
pub struct ImpactRadiusArgs<'a> {
    pub symbol: &'a str,
    /// Token-estimate ceiling: sheds to a file-level, then
    /// module-level summary on a synthetic hub's large blast radius.
    pub max_tokens: Option<usize>,
}

/// Arguments for `weave_search_semantic` (CORE-03/SEC-04, feature `vector`).
#[cfg(feature = "vector")]
pub struct SemanticSearchArgs<'a> {
    pub query: &'a str,
    pub limit: usize,
}

/// Arguments for `weave_explore` (P10.2): composes repo map, file API,
/// call trace, impact radius, and an exact source excerpt behind one
/// budget, as one additional tool alongside — not replacing — the four
/// narrow pull-style ones.
pub struct ExploreArgs<'a> {
    /// `Some` orients around one symbol (file API + call trace + impact
    /// radius + source excerpt); `None` falls back to a module-level
    /// repo map orientation.
    pub symbol: Option<&'a str>,
    /// Token-estimate ceiling: sheds the source excerpt, then the call
    /// trace, then the impact radius (in that order) before giving up
    /// and reporting the actual resident size over budget.
    pub max_tokens: Option<usize>,
}

/// Output of `weave_explore`.
pub struct ExploreResult {
    pub text: String,
}

/// Arguments for `weave_find_all` (P10.4, feature `fts`): exhaustive,
/// deterministic symbol-body text search, grouped by enclosing indexed
/// symbol since each indexed FTS row already *is* one symbol's own body
/// span — never a ranked top-N (`weave_search_semantic`'s own job).
#[cfg(feature = "fts")]
pub struct FindAllArgs<'a> {
    pub pattern: &'a str,
    /// Only symbols whose path starts with this prefix.
    pub path: Option<&'a str>,
    /// Only symbols in a file of this language (e.g. `"rust"`), matched
    /// case-insensitively against a small extension map local to this
    /// crate (never `weave-graph-parse`'s own `Language` enum — the
    /// wrong dependency direction).
    pub language: Option<&'a str>,
    /// Only symbols whose `kind` equals this exactly (e.g. `"function"`).
    pub kind: Option<&'a str>,
    /// Hard cap on displayed hits — `total_matches` still reports the
    /// full exhaustive count even when the rendered list is shorter.
    pub limit: usize,
    pub max_tokens: Option<usize>,
}

/// Output of `weave_find_all`.
#[cfg(feature = "fts")]
pub struct FindAllResult {
    pub text: String,
    pub total_matches: usize,
}

/// Word-count token estimate: a whitespace-split count — the same
/// class of estimate the project's own token-reduction claims already
/// rely on elsewhere. Deliberately NOT a real tokenizer and never claimed
/// to be one; it only needs to bound output size roughly.
pub(crate) fn estimate_tokens(text: &str) -> usize {
    text.split_whitespace().count()
}

/// A rendered response fits its budget (or there is no budget).
pub(crate) fn under_budget(text: &str, max_tokens: Option<usize>) -> bool {
    match max_tokens {
        Some(max) => estimate_tokens(text) <= max,
        None => true,
    }
}

/// One symbol entry in a wiring card — the ~60-token building block.
#[derive(Debug, Clone, PartialEq)]
pub struct SymbolEntry {
    pub kind: String,
    pub symbol: String,
    /// `L{start}-{end}` format, e.g. `L10-25`.
    pub span: String,
    pub signature: String,
}

impl SymbolEntry {
    pub(crate) fn from_node(n: &Node) -> Self {
        Self {
            kind: n.kind.clone(),
            symbol: n.symbol.clone(),
            span: format!("L{}-{}", n.line_start, n.line_end),
            signature: n.signature.clone(),
        }
    }
}

/// Per-file wiring card returned by `weave_file_api`.
#[derive(Debug, Clone)]
pub struct WiringCard {
    pub path: String,
    pub symbols: Vec<SymbolEntry>,
}

/// Output of `weave_repo_map` — progressive orientation, ~200 tokens.
pub struct RepoMapResult {
    pub text: String,
}

/// Output of `weave_file_api`.
pub struct FileApiResult {
    pub cards: Vec<WiringCard>,
}

/// Output of `weave_trace_calls`.
pub struct TraceCallsResult {
    pub text: String,
}

/// Output of `weave_impact_radius`.
pub struct ImpactRadiusResult {
    pub symbol_count: usize,
    pub text: String,
}

/// Resolves `symbol` to a `NodeId` — exact match against `all_nodes`
/// first (the first match, lowest id, on a collision), then
/// `weave_graph_core::resolve`'s deterministic fallback chain
/// (case-insensitive, short-name, edit-distance suggestions) on a miss.
pub(crate) fn resolve_symbol(nodes: &[Node], symbol: &str) -> Result<NodeId, Vec<String>> {
    core_resolve_symbol(nodes, symbol)
}

#[cfg(test)]
mod tests;
