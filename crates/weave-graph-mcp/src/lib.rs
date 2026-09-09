#![deny(unsafe_code)]
//! MCP transport adapter and tool surface. Depends only on
//! `weave-graph-core`. All four tools are 100% deterministic — no LLM,
//! no network, regardless of which features are compiled in (`plan.md` §1.5).

mod error;
mod file_api;
mod handler;
mod impact_radius;
pub mod protocol;
mod repo_map;
mod tools;
mod trace_calls;
mod transport;

pub use error::McpError;
pub use file_api::weave_file_api;
pub use handler::McpHandler;
pub use impact_radius::weave_impact_radius;
pub use protocol::{
    CallToolResult, JsonRpcError, JsonRpcRequest, JsonRpcResponse, TextContent, ToolDefinition,
};
pub use repo_map::weave_repo_map;
pub use tools::{
    FileApiArgs, FileApiResult, ImpactRadiusArgs, ImpactRadiusResult, RepoMapArgs, RepoMapResult,
    SymbolEntry, TraceCallsArgs, TraceCallsResult, WiringCard,
};
pub use trace_calls::weave_trace_calls;
pub use transport::{HttpTransport, McpTransport, StdioTransport, validate_loopback_bind};
