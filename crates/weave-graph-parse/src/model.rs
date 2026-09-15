/// Per-file wiring card: a symbol's signature and exact line range, cheap
/// enough for an AI agent to slice-edit from rather than ingesting the
/// whole file. `moniker` is this crate's cross-file key — see the
/// `moniker` module docs for what it deliberately is not.
#[derive(Debug, Clone, PartialEq)]
pub struct WiringCard {
    pub moniker: String,
    /// Dotted scope path, e.g. `Bar::baz` (Rust) or `Bar.baz` (Python/JS) —
    /// distinguishes same-named methods on different types within a file.
    pub symbol: String,
    pub kind: SymbolKind,
    pub line_start: u32,
    pub line_end: u32,
    pub signature: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Function,
    Method,
    Struct,
    Class,
    Interface,
    Impl,
}

impl SymbolKind {
    /// `nodes.kind` stays free-form TEXT in the schema — this is that text.
    pub fn as_str(self) -> &'static str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Struct => "struct",
            SymbolKind::Class => "class",
            SymbolKind::Interface => "interface",
            SymbolKind::Impl => "impl",
        }
    }
}

/// A call site, not yet resolved to a target. `is_member_call`
/// distinguishes `foo()` from `x.foo()` — the latter can't resolve to a
/// single static target without type inference, so it always becomes
/// `CALLS_DYNAMIC` (see `resolve` module).
#[derive(Debug, Clone, PartialEq)]
pub struct RawCall {
    pub caller_moniker: String,
    pub callee_name: String,
    pub is_member_call: bool,
}

/// `IMPORTS`/`INHERITS`/`IMPLEMENTS` structural edges.
/// `target_name` is the raw textual reference (an import path, a base
/// class, a trait); these resolve the same way `RawCall` does.
#[derive(Debug, Clone, PartialEq)]
pub struct RawStructuralEdge {
    pub source_moniker: String,
    pub target_name: String,
    pub kind: StructuralEdgeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuralEdgeKind {
    Imports,
    Inherits,
    Implements,
}

impl StructuralEdgeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            StructuralEdgeKind::Imports => "IMPORTS",
            StructuralEdgeKind::Inherits => "INHERITS",
            StructuralEdgeKind::Implements => "IMPLEMENTS",
        }
    }
}

/// Everything extracted from one source file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedFile {
    pub symbols: Vec<WiringCard>,
    pub calls: Vec<RawCall>,
    pub structural_edges: Vec<RawStructuralEdge>,
}

#[cfg(test)]
mod tests;
