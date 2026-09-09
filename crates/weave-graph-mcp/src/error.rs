use std::io;

use thiserror::Error;
use weave_graph_core::StorageError;

/// MCP protocol and transport errors.
#[derive(Debug, Error)]
pub enum McpError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("Protocol error: {0}")]
    Protocol(String),
    #[error("Security error: {0}")]
    Security(String),
}
