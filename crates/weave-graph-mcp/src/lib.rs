#![deny(unsafe_code)]
//! MCP transport adapter and tool surface. Depends only on
//! `weave-graph-core`. All four tools are 100% deterministic — no LLM,
//! no network, regardless of which features are compiled in (`plan.md` §1.5).

mod error;
mod file_api;
mod handler;
mod impact_radius;
#[cfg(feature = "notes")]
mod notes;
#[cfg(feature = "policy-lint")]
mod policy_lint;
pub mod protocol;
mod repo_map;
#[cfg(feature = "vector")]
mod search_semantic;
mod tools;
mod trace_calls;
mod transport;

pub use error::McpError;
pub use file_api::weave_file_api;
pub use handler::McpHandler;
pub use impact_radius::weave_impact_radius;
#[cfg(feature = "policy-lint")]
pub use policy_lint::weave_policy_lint;
pub use protocol::{
    CallToolResult, JsonRpcError, JsonRpcRequest, JsonRpcResponse, TextContent, ToolDefinition,
};
pub use repo_map::weave_repo_map;
#[cfg(feature = "vector")]
pub use search_semantic::weave_search_semantic;
#[cfg(feature = "vector")]
pub use tools::SemanticSearchArgs;
pub use tools::{
    FileApiArgs, FileApiResult, ImpactRadiusArgs, ImpactRadiusResult, RepoMapArgs, RepoMapResult,
    SymbolEntry, TraceCallsArgs, TraceCallsResult, WiringCard,
};
pub use trace_calls::weave_trace_calls;
pub use transport::{HttpTransport, McpTransport, StdioTransport, validate_loopback_bind};
