#![deny(unsafe_code)]
//! Tree-sitter AST extraction, wiring-card generation, and language
//! adapters. Depends only on `weave-graph-core`.

pub mod contract;
mod extract;
mod language;
#[cfg(feature = "docs")]
pub mod markdown;
mod model;
pub mod moniker;
mod parser;
mod resolve;

pub use language::Language;
pub use model::{
    ParsedFile, RawCall, RawStructuralEdge, StructuralEdgeKind, SymbolKind, WiringCard,
};
pub use parser::{ParseError, SourceParser, parse_file};
pub use resolve::{ProjectIndex, ResolutionKind, ResolvedEdge};
