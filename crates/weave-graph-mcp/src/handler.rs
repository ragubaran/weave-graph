use serde_json::{Value, json};
use weave_graph_core::{CsrGraph, Storage, StorageError};

use crate::file_api::weave_file_api;
use crate::impact_radius::weave_impact_radius;
use crate::protocol::{CallToolResult, JsonRpcRequest, JsonRpcResponse, ToolDefinition};
use crate::repo_map::weave_repo_map;
use crate::tools::{FileApiArgs, ImpactRadiusArgs, RepoMapArgs, TraceCallsArgs};
use crate::trace_calls::weave_trace_calls;

/// Core MCP message processor executing against storage and CSR index.
pub struct McpHandler<'a> {
    storage: &'a dyn Storage,
    csr: CsrGraph,
}

impl<'a> McpHandler<'a> {
    /// Creates a handler wrapping a Storage backend, loading CSR adjacency.
    pub fn new(storage: &'a dyn Storage) -> Result<Self, StorageError> {
        let csr = CsrGraph::load(storage)?;
        Ok(Self { storage, csr })
    }

    /// Creates a handler with an existing preloaded CSR index.
    pub fn with_csr(storage: &'a dyn Storage, csr: CsrGraph) -> Self {
        Self { storage, csr }
    }

    /// Handles a raw JSON-RPC string message and returns a response if required.
    pub fn handle_message(&self, raw: &str) -> Option<JsonRpcResponse> {
        let req: JsonRpcRequest = match serde_json::from_str(raw) {
            Ok(r) => r,
            Err(e) => {
                return Some(JsonRpcResponse::error(
                    None,
                    -32700,
                    format!("Parse error: {e}"),
                ));
            }
        };

        match req.method.as_str() {
            "initialize" => Some(JsonRpcResponse::success(req.id, self.handle_initialize())),
            "notifications/initialized" => None,
            "ping" => Some(JsonRpcResponse::success(req.id, json!({}))),
            "tools/list" => Some(JsonRpcResponse::success(req.id, self.handle_tools_list())),
            "tools/call" => Some(self.handle_tools_call(req.id, req.params)),
            _ => {
                if req.id.is_some() {
                    Some(JsonRpcResponse::error(
                        req.id,
                        -32601,
                        format!("Method not found: {}", req.method),
                    ))
                } else {
                    None
                }
            }
        }
    }

    fn handle_initialize(&self) -> Value {
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "weave",
                "version": "0.1.0"
            }
        })
    }

    fn handle_tools_list(&self) -> Value {
        let tools = vec![
            ToolDefinition {
                name: "weave_repo_map".to_string(),
                description:
                    "Progressive architectural orientation of active modules (~200 tokens)."
                        .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "max_files": { "type": "integer", "description": "Max number of files to surface" },
                        "module": { "type": "boolean", "description": "Module-level orientation: one line per Louvain module (label, file count, symbol count, cross-edges, member files)" }
                    }
                }),
            },
            ToolDefinition {
                name: "weave_file_api".to_string(),
                description: "Returns micro wiring cards for requested files (~60 tokens/file)."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "paths": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "File paths to generate wiring cards for"
                        }
                    },
                    "required": ["paths"]
                }),
            },
            ToolDefinition {
                name: "weave_trace_calls".to_string(),
                description: "Traverses incoming/outgoing call chains up to N hops.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "symbol": { "type": "string", "description": "Symbol name to trace" },
                        "depth": { "type": "integer", "description": "Traversal depth in hops" }
                    },
                    "required": ["symbol"]
                }),
            },
            ToolDefinition {
                name: "weave_impact_radius".to_string(),
                description: "Computes topological blast radius for proposed changes.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "symbol": { "type": "string", "description": "Root symbol being changed" }
                    },
                    "required": ["symbol"]
                }),
            },
        ];

        json!({ "tools": tools })
    }

    fn handle_tools_call(&self, id: Option<Value>, params: Option<Value>) -> JsonRpcResponse {
        let params = match params {
            Some(p) => p,
            None => {
                return JsonRpcResponse::error(id, -32602, "Missing params for tools/call");
            }
        };

        let name = match params.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => {
                return JsonRpcResponse::error(id, -32602, "Missing tool name in tools/call");
            }
        };

        let args = params.get("arguments").cloned().unwrap_or(json!({}));

        let tool_result = match name {
            "weave_repo_map" => self.call_repo_map(&args),
            "weave_file_api" => self.call_file_api(&args),
            "weave_trace_calls" => self.call_trace_calls(&args),
            "weave_impact_radius" => self.call_impact_radius(&args),
            _ => CallToolResult::err(format!("Unknown tool: {name}")),
        };

        let result_val = serde_json::to_value(&tool_result).unwrap_or(json!({
            "content": [{"type": "text", "text": "Serialization failed"}],
            "isError": true
        }));

        JsonRpcResponse::success(id, result_val)
    }

    fn call_repo_map(&self, args: &Value) -> CallToolResult {
        let max_files = args
            .get("max_files")
            .and_then(|v| v.as_u64())
            .map(|d| d as usize)
            .unwrap_or(50);
        let module = args.get("module").and_then(|v| v.as_bool());
        let res = weave_repo_map(self.storage, &self.csr, RepoMapArgs { max_files, module });
        CallToolResult::ok(res.text)
    }

    fn call_file_api(&self, args: &Value) -> CallToolResult {
        let paths_vec: Vec<&str> = match args.get("paths").and_then(|v| v.as_array()) {
            Some(arr) => arr.iter().filter_map(|v| v.as_str()).collect(),
            None => vec![],
        };
        let res = weave_file_api(self.storage, FileApiArgs { paths: &paths_vec });
        let mut out = String::new();
        for card in &res.cards {
            out.push_str(&format!("{}:\n", card.path));
            for sym in &card.symbols {
                out.push_str(&format!("  {} [{}] {}\n", sym.symbol, sym.kind, sym.span));
            }
        }
        CallToolResult::ok(out)
    }

    fn call_trace_calls(&self, args: &Value) -> CallToolResult {
        let symbol = match args.get("symbol").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return CallToolResult::err("Missing 'symbol' parameter"),
        };
        let depth = args
            .get("depth")
            .and_then(|v| v.as_u64())
            .map(|d| d as u32)
            .unwrap_or(2);
        let res = weave_trace_calls(self.storage, &self.csr, TraceCallsArgs { symbol, depth });
        CallToolResult::ok(res.text)
    }

    fn call_impact_radius(&self, args: &Value) -> CallToolResult {
        let symbol = match args.get("symbol").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return CallToolResult::err("Missing 'symbol' parameter"),
        };
        let res = weave_impact_radius(self.storage, &self.csr, ImpactRadiusArgs { symbol });
        CallToolResult::ok(res.text)
    }
}

#[cfg(test)]
mod tests;
