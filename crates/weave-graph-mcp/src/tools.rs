use weave_graph_core::{Node, NodeId};

/// Arguments for `weave_repo_map`. Caps output to stay within ~200 tokens.
pub struct RepoMapArgs {
    /// Max number of files to surface (default: 50).
    pub max_files: usize,
}

impl Default for RepoMapArgs {
    fn default() -> Self {
        Self { max_files: 50 }
    }
}

/// Arguments for `weave_file_api`.
pub struct FileApiArgs<'a> {
    pub paths: &'a [&'a str],
}

/// Arguments for `weave_trace_calls`.
pub struct TraceCallsArgs<'a> {
    pub symbol: &'a str,
    /// Max hop depth for both incoming and outgoing traversal.
    pub depth: u32,
}

/// Arguments for `weave_impact_radius`.
pub struct ImpactRadiusArgs<'a> {
    pub symbol: &'a str,
}

/// One symbol entry in a wiring card — the ~60-token building block.
#[derive(Debug, PartialEq)]
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
#[derive(Debug)]
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

/// Resolves `symbol` to a NodeId by exact match against `all_nodes`.
/// Returns the first match (lowest id) when multiple definitions exist.
pub(crate) fn resolve_symbol(nodes: &[Node], symbol: &str) -> Option<NodeId> {
    nodes.iter().find(|n| n.symbol == symbol).map(|n| n.id)
}

#[cfg(test)]
mod tests;
