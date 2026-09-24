use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};
#[cfg(feature = "policy-lint")]
use weave_graph_core::Storage;
#[cfg(feature = "rbac")]
use weave_graph_core::rbac::RbacGuard;
use weave_graph_core::{CsrGraph, Node, StorageError};

#[cfg(feature = "rbac")]
type GuardRef<'a> = Option<&'a RbacGuard>;
#[cfg(feature = "rbac")]
type TokenAuthProvider = std::rc::Rc<dyn Fn(&str) -> Option<RbacGuard>>;
#[cfg(not(feature = "rbac"))]
type GuardRef<'a> = Option<&'a ()>;

use crate::explore::weave_explore;
use crate::file_api::weave_file_api;
#[cfg(feature = "fts")]
use crate::find_all::weave_find_all;
use crate::freshness::{Freshness, check_freshness};
use crate::impact_radius::weave_impact_radius;
#[cfg(feature = "notes")]
use crate::notes::{PinNoteArgs, weave_pin_note, weave_recall_notes};
use crate::protocol::{
    CallToolResult, JsonRpcRequest, JsonRpcResponse, TextContent, ToolDefinition,
};
use crate::repo_map::weave_repo_map;
#[cfg(feature = "fts")]
use crate::tools::FindAllArgs;
use crate::tools::{ExploreArgs, FileApiArgs, ImpactRadiusArgs, RepoMapArgs, TraceCallsArgs};
use crate::trace_calls::weave_trace_calls;
use crate::verify::{VerifyArgs, weave_verify};

/// Mirrors `weave-graph-cli::watch::PendingMarker`'s JSON shape without
/// depending on that crate (wrong dependency direction) — `(blast_radius,
/// files)`, or `None` when absent or unparseable.
fn read_pending_marker(weave_dir: &std::path::Path) -> Option<(usize, Vec<String>)> {
    let content = std::fs::read_to_string(weave_dir.join("pending-manual-reindex")).ok()?;
    let value: Value = serde_json::from_str(&content).ok()?;
    let blast_radius = value.get("blast_radius")?.as_u64()? as usize;
    let files = value
        .get("files")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    Some((blast_radius, files))
}

/// Mirrors `weave-graph-cli::watch`'s `watch-in-flight` marker shape (a
/// plain JSON array of paths); empty when absent or unparseable.
fn read_in_flight(weave_dir: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(weave_dir.join("watch-in-flight"))
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

/// A tool's own text sometimes carries a well-known failure prefix
/// (`"symbol not found: "`, `"error: "`) — surfaced as a real MCP
/// `isError: true` result instead of a silent success, so an agent can
/// tell "nothing matched" from "here's the answer" from the envelope
/// alone, not by parsing text. Same convention `call_pin_note` already
/// used for its own `"error:"` prefix, applied consistently here too.
fn as_tool_result(text: String) -> CallToolResult {
    if text.starts_with("symbol not found:") || text.starts_with("error:") {
        CallToolResult::err(text)
    } else {
        CallToolResult::ok(text)
    }
}

/// Core MCP message processor executing against its own storage and CSR
/// index: the handler **owns** its `SqliteStorage` behind
/// `RefCell` (both transports are single-threaded) and, when it opened a
/// file-backed database, live-reloads on external reindexes.
pub struct McpHandler {
    storage: RefCell<weave_graph_store_sqlite::SqliteStorage>,
    csr: RefCell<CsrGraph>,
    weave_dir: Option<std::path::PathBuf>,
    /// `Some` = file-backed (reload enabled); `None` = in-memory (never reloads).
    db_path: Option<std::path::PathBuf>,
    read_only: bool,
    last_mtime: RefCell<Option<SystemTime>>,
    last_checked: Cell<Option<Instant>>,
    recheck_interval: Duration,
    /// The same `RbacGuard` `weave query`/`report`/`export` use,
    /// bound once per server session (`with_identity`) rather than
    /// per-call — this handler already models one session as one
    /// identity.
    #[cfg(feature = "rbac")]
    rbac_guard: Option<RbacGuard>,
    #[cfg(feature = "rbac")]
    token_auth_provider: Option<TokenAuthProvider>,
    #[cfg(feature = "rbac")]
    require_auth: bool,
}

impl McpHandler {
    /// Creates a handler wrapping a Storage backend, loading CSR adjacency.
    /// In-memory backends have no reload path (nothing to re-stat).
    pub fn new(storage: weave_graph_store_sqlite::SqliteStorage) -> Result<Self, StorageError> {
        let csr = CsrGraph::load(&storage)?;
        Ok(Self {
            storage: RefCell::new(storage),
            csr: RefCell::new(csr),
            weave_dir: None,
            db_path: None,
            read_only: false,
            last_mtime: RefCell::new(None),
            last_checked: std::cell::Cell::new(None),
            recheck_interval: Duration::from_millis(500),
            #[cfg(feature = "rbac")]
            rbac_guard: None,
            #[cfg(feature = "rbac")]
            token_auth_provider: None,
            #[cfg(feature = "rbac")]
            require_auth: false,
        })
    }

    /// Creates a handler over the database file at `path`, with live
    /// reload: every message first checks the file's mtime (bounded to
    /// once per `recheck_interval`) and, on change, **fully closes and
    /// re-opens** the connection — an already-open `rusqlite::Connection`
    /// stays pinned to the old inode after another process `rename()`s a
    /// new file onto the path, so a fresh open is the only correct reload.
    pub fn open(db_path: &Path) -> Result<Self, StorageError> {
        Self::open_with_mode(db_path, false)
    }

    /// [`open`] with an explicit reopen mode: `read_only = true` reopens
    /// in the shared-snapshot mode a network-mounted database requires.
    pub fn open_with_mode(db_path: &Path, read_only: bool) -> Result<Self, StorageError> {
        let storage = if read_only {
            weave_graph_store_sqlite::SqliteStorage::open_read_only(db_path)?
        } else {
            weave_graph_store_sqlite::SqliteStorage::open(db_path)?
        };
        let csr = CsrGraph::load(&storage)?;
        let last_mtime = std::fs::metadata(db_path)
            .ok()
            .and_then(|m| m.modified().ok());
        Ok(Self {
            storage: RefCell::new(storage),
            csr: RefCell::new(csr),
            weave_dir: None,
            db_path: Some(db_path.to_path_buf()),
            read_only,
            last_mtime: RefCell::new(last_mtime),
            last_checked: std::cell::Cell::new(None),
            recheck_interval: Duration::from_millis(500),
            #[cfg(feature = "rbac")]
            rbac_guard: None,
            #[cfg(feature = "rbac")]
            token_auth_provider: None,
            #[cfg(feature = "rbac")]
            require_auth: false,
        })
    }

    /// Binds this session to one `RbacGuard` — the
    /// same guard `weave query`/`report`/`export` build from `--as
    /// <subject>`, so a `weave serve --mcp --as <subject>` session masks
    /// consistently with the CLI. A no-op builder when never called (the
    /// default `new`/`open` path stays unmasked, matching every other
    /// feature's "compiled in but unused = unchanged" isolation).
    #[cfg(feature = "rbac")]
    pub fn with_identity(mut self, guard: RbacGuard) -> Self {
        self.rbac_guard = Some(guard);
        self
    }

    #[cfg(feature = "rbac")]
    pub fn with_token_auth(mut self, provider: TokenAuthProvider) -> Self {
        self.token_auth_provider = Some(provider);
        self
    }

    #[cfg(feature = "rbac")]
    pub fn with_require_auth(mut self, require_auth: bool) -> Self {
        self.require_auth = require_auth;
        self
    }

    /// Opts into surfacing `watch`'s staleness markers —
    /// `.weave/pending-manual-reindex` and `.weave/watch-in-flight` — in
    /// every tool response when present. A no-op builder when the caller
    /// never sets it (the default `new`/`with_csr` path), so every existing
    /// caller and test is unaffected.
    pub fn with_weave_dir(mut self, weave_dir: std::path::PathBuf) -> Self {
        self.weave_dir = Some(weave_dir);
        self
    }

    /// Overrides the live-reload recheck bound. The
    /// default 500ms keeps the mtime `stat()` off the fast path between
    /// reindexes; tests drive it to zero to reload deterministically.
    pub fn with_recheck_interval(mut self, interval: Duration) -> Self {
        self.recheck_interval = interval;
        self
    }

    /// Bounded mtime check at the top of every message.
    /// When another process reindexed (Core Invariant 2's atomic rename
    /// always moves the mtime), the handler **fully closes and reopens**
    /// its connection — POSIX rename never retargets an already-open fd —
    /// and rebuilds the CSR. Failure to reopen keeps the old state (and a
    /// stderr note); it never panics or loses the session.
    fn maybe_reload(&self) {
        let Some(db_path) = &self.db_path else {
            return;
        };
        let now = Instant::now();
        if let Some(last) = self.last_checked.get()
            && now.duration_since(last) < self.recheck_interval
        {
            return;
        }
        self.last_checked.set(Some(now));
        // A vanished file is transient (mid-swap or removed) — never reopen
        // against it: `SqliteStorage::open` would recreate an EMPTY
        // database and wipe the handler's good snapshot.
        let Ok(meta) = std::fs::metadata(db_path) else {
            return;
        };
        let mtime = meta.modified().ok();
        if mtime == *self.last_mtime.borrow() {
            return;
        }
        let reopened = if self.read_only {
            weave_graph_store_sqlite::SqliteStorage::open_read_only(db_path)
        } else {
            weave_graph_store_sqlite::SqliteStorage::open(db_path)
        };
        match reopened {
            Ok(new_storage) => match CsrGraph::load(&new_storage) {
                Ok(csr) => {
                    *self.storage.borrow_mut() = new_storage;
                    *self.csr.borrow_mut() = csr;
                    *self.last_mtime.borrow_mut() = std::fs::metadata(db_path)
                        .ok()
                        .and_then(|m| m.modified().ok());
                }
                Err(e) => eprintln!("mcp: reload skipped (CSR rebuild failed): {e}"),
            },
            Err(e) => eprintln!("mcp: reload skipped (reopen failed): {e}"),
        }
    }

    /// Handles a raw JSON-RPC string message and returns a response if required.
    pub fn handle_message(&self, raw: &str) -> Option<JsonRpcResponse> {
        self.maybe_reload();
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

    /// Applies a transport credential as request metadata before dispatch.
    pub fn handle_message_with_token(
        &self,
        raw: &str,
        token: Option<&str>,
    ) -> Option<JsonRpcResponse> {
        let Some(token) = token else {
            return self.handle_message(raw);
        };
        let Ok(mut request) = serde_json::from_str::<Value>(raw) else {
            return self.handle_message(raw);
        };
        if request.get("method").and_then(Value::as_str) != Some("tools/call") {
            return self.handle_message(raw);
        }
        let Some(params) = request.get_mut("params").and_then(Value::as_object_mut) else {
            return self.handle_message(raw);
        };
        let arguments = params.entry("arguments").or_insert_with(|| json!({}));
        let Some(arguments) = arguments.as_object_mut() else {
            return self.handle_message(raw);
        };
        arguments.insert("_meta".to_string(), json!({ "token": token }));
        serde_json::to_string(&request)
            .ok()
            .and_then(|request| self.handle_message(&request))
    }

    /// Checks whether a transport credential can satisfy required RBAC auth.
    pub fn accepts_request_token(&self, token: Option<&str>) -> bool {
        #[cfg(feature = "rbac")]
        {
            if !self.require_auth || self.rbac_guard.is_some() {
                true
            } else {
                token.is_some_and(|token| {
                    self.token_auth_provider
                        .as_ref()
                        .and_then(|provider| provider(token))
                        .is_some()
                })
            }
        }
        #[cfg(not(feature = "rbac"))]
        {
            let _ = token;
            true
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
                        "module": { "type": "boolean", "description": "Module-level orientation: one line per Louvain module (label, file count, symbol count, cross-edges, member files)" },
                        "max_tokens": { "type": "integer", "description": "Token-estimate ceiling: sheds lines to fit (replaces max_files truncation)" }
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
                        },
                        "max_tokens": { "type": "integer", "description": "Token-estimate ceiling: sheds tiers (full cards → symbol names → counts) on overflow" }
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
                        "depth": { "type": "integer", "description": "Traversal depth in hops" },
                        "max_tokens": { "type": "integer", "description": "Token-estimate ceiling: truncates chains with explicit 'and N more' markers" },
                        "precise_only": { "type": "boolean", "description": "Drop heuristically-resolved callers from the incoming chain (P10.5); the outgoing chain has no per-edge confidence to filter on" }
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
                        "symbol": { "type": "string", "description": "Root symbol being changed" },
                        "max_tokens": { "type": "integer", "description": "Token-estimate ceiling: sheds to a file-level, then module-level summary on overflow" }
                    },
                    "required": ["symbol"]
                }),
            },
            #[cfg(feature = "notes")]
            ToolDefinition {
                name: "weave_pin_note".to_string(),
                description: "Pins a note onto a symbol so a later session's agent sees it (feature: notes).".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "symbol": { "type": "string", "description": "Symbol name to pin the note onto" },
                        "text": { "type": "string", "description": "The note text" },
                        "tier": { "type": "string", "description": "\"ephemeral\" (24h TTL, default) or \"crystallized\" (kept)" },
                        "kind": { "type": "string", "description": "Note category, e.g. \"arch_decision\"" }
                    },
                    "required": ["symbol", "text"]
                }),
            },
            #[cfg(feature = "notes")]
            ToolDefinition {
                name: "weave_recall_notes".to_string(),
                description: "Recalls live pinned notes (expired ephemerals filtered at read time; orphaned notes reported).".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            #[cfg(feature = "vector")]
            ToolDefinition {
                name: "weave_search_semantic".to_string(),
                description: "Binary-ANN-then-int8-rerank semantic search over AST-bounded chunks (feature: vector).".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Natural-language or code-shaped search query" },
                        "limit": { "type": "integer", "description": "Max hits to return (default: 5)" }
                    },
                    "required": ["query"]
                }),
            },
            #[cfg(feature = "policy-lint")]
            ToolDefinition {
                name: "weave_policy_lint".to_string(),
                description: "Evaluates declared architectural boundaries (.weave/policy.yaml) against the indexed graph (feature: policy-lint).".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            ToolDefinition {
                name: "weave_check_freshness".to_string(),
                description: "Reconciles the indexed graph against the live working tree (behind HEAD, uncommitted edits, or fresh) before you trust other tools' answers.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            ToolDefinition {
                name: "weave_explore".to_string(),
                description: "Composes the repo map, file API, call trace, impact radius, and an exact source excerpt behind one token budget (P10.2) — one additional orientation tool, not a replacement for the four narrow ones.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "symbol": { "type": "string", "description": "Symbol to orient around; omit for a module-level repo overview" },
                        "max_tokens": { "type": "integer", "description": "Token-estimate ceiling: sheds the source excerpt, then the call trace, then the impact radius before reporting the actual resident size" }
                    }
                }),
            },
            #[cfg(feature = "fts")]
            ToolDefinition {
                name: "weave_find_all".to_string(),
                description: "Exhaustive, deterministic text search over every indexed symbol's body/doc comment, grouped by enclosing symbol (feature: fts) — never a ranked top-N.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string", "description": "Text to search for" },
                        "path": { "type": "string", "description": "Only symbols whose path starts with this prefix" },
                        "language": { "type": "string", "description": "Only symbols in a file of this language, e.g. \"rust\"" },
                        "kind": { "type": "string", "description": "Only symbols of this exact kind, e.g. \"function\"" },
                        "limit": { "type": "integer", "description": "Max hits to render (default: 50); the full exhaustive count is always reported" },
                        "max_tokens": { "type": "integer", "description": "Token-estimate ceiling on the rendered list" }
                    },
                    "required": ["pattern"]
                }),
            },
            ToolDefinition {
                name: "weave_verify".to_string(),
                description: "Checks one file for phantom symbols (calls/references with no matching indexed symbol) before you present an edit — the fast, storage-only slice of `weave verify`.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "file": { "type": "string", "description": "Indexed file path to verify" },
                        "range": { "type": "array", "items": { "type": "integer" }, "description": "Optional [start, end] line range, display-only" }
                    },
                    "required": ["file"]
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
        #[cfg(feature = "rbac")]
        let mut args = args;

        #[cfg(feature = "rbac")]
        let mut resolved_guard = None;

        #[cfg(feature = "rbac")]
        {
            let token = args
                .as_object_mut()
                .and_then(|obj| obj.remove("_meta"))
                .and_then(|meta| meta.get("token").cloned())
                .and_then(|v| v.as_str().map(String::from));

            if let Some(t) = token
                && self.rbac_guard.is_none()
                && let Some(provider) = &self.token_auth_provider
            {
                resolved_guard = provider(&t);
            }

            if self.require_auth && self.rbac_guard.is_none() && resolved_guard.is_none() {
                return JsonRpcResponse::error(
                    id,
                    -32000,
                    "Unauthorized: valid _meta.token required (SEC-07)",
                );
            }
        }

        #[cfg(feature = "rbac")]
        let guard_ref = resolved_guard.as_ref().or(self.rbac_guard.as_ref());
        #[cfg(not(feature = "rbac"))]
        let guard_ref = None;

        let mut tool_result = match name {
            "weave_repo_map" => self.call_repo_map(&args, guard_ref),
            "weave_file_api" => self.call_file_api(&args, guard_ref),
            "weave_trace_calls" => self.call_trace_calls(&args, guard_ref),
            "weave_impact_radius" => self.call_impact_radius(&args, guard_ref),
            "weave_explore" => self.call_explore(&args, guard_ref),
            #[cfg(feature = "fts")]
            "weave_find_all" => self.call_find_all(&args, guard_ref),
            #[cfg(feature = "notes")]
            "weave_pin_note" => self.call_pin_note(&args),
            #[cfg(feature = "notes")]
            "weave_recall_notes" => CallToolResult::ok(weave_recall_notes(&*self.storage.borrow())),
            #[cfg(feature = "vector")]
            "weave_search_semantic" => self.call_search_semantic(&args, guard_ref),
            #[cfg(feature = "policy-lint")]
            "weave_policy_lint" => self.call_policy_lint(guard_ref),
            "weave_check_freshness" => self.call_check_freshness(),
            "weave_verify" => self.call_verify(&args),
            _ => CallToolResult::err(format!("Unknown tool: {name}")),
        };
        self.append_watch_staleness(&mut tool_result);

        let result_val = serde_json::to_value(&tool_result).unwrap_or(json!({
            "content": [{"type": "text", "text": "Serialization failed"}],
            "isError": true
        }));

        JsonRpcResponse::success(id, result_val)
    }

    /// Appends a warning block for either `watch` staleness
    /// marker when `weave_dir` is set and one is present — a no-op (and a
    /// byte-identical response) whenever `weave_dir` is unset, the `watch`
    /// feature isn't enabled, or neither marker exists. Deliberately reads
    /// the marker files directly rather than depending on `weave-graph-cli`
    /// (wrong dependency direction) — the JSON shape is the only contract.
    fn append_watch_staleness(&self, result: &mut CallToolResult) {
        let Some(weave_dir) = &self.weave_dir else {
            return;
        };
        if let Some((blast_radius, files)) = read_pending_marker(weave_dir) {
            result.content.push(TextContent::new(format!(
                "⚠️ {blast_radius} symbols' worth of blast radius pending — run `weave index` to refresh ({} file(s): {})",
                files.len(),
                files.join(", ")
            )));
        }
        let in_flight = read_in_flight(weave_dir);
        if !in_flight.is_empty() {
            result.content.push(TextContent::new(format!(
                "ℹ️ {} file(s) just changed, not yet reindexed (still inside the debounce window): {}",
                in_flight.len(),
                in_flight.join(", ")
            )));
        }
    }

    /// `weave_check_freshness` (`docs/sum_feat.md` P10.3): a dedicated,
    /// explicit freshness check, distinct from `append_watch_staleness`'s
    /// passive marker-file read on every call — this one actively
    /// reconciles the indexed commit against the working tree via `git`,
    /// so it still reports something meaningful when `watch` mode never
    /// ran. Never silently claims fresh when it can't tell (`Unknown`).
    fn call_check_freshness(&self) -> CallToolResult {
        let Some(weave_dir) = &self.weave_dir else {
            return CallToolResult::ok(
                "Freshness unknown: no `.weave` directory configured for this session.".to_string(),
            );
        };
        let text = match check_freshness(weave_dir) {
            Freshness::Fresh => {
                "✅ Fresh: indexed commit matches HEAD, working tree clean.".to_string()
            }
            Freshness::Dirty => "⚠️ Dirty: indexed commit matches HEAD, but the working tree has \
                 uncommitted changes not reflected in the graph — run `weave index`."
                .to_string(),
            Freshness::Behind {
                indexed_sha,
                head_sha,
            } => format!(
                "⚠️ Behind: indexed {} but HEAD is {head_sha} — run `weave index`.",
                indexed_sha.as_deref().unwrap_or("nothing yet")
            ),
            Freshness::Unknown => {
                "ℹ️ Freshness unknown: this directory isn't a Git repository (or `git` \
                 isn't on PATH)."
                    .to_string()
            }
        };
        CallToolResult::ok(text)
    }

    /// `weave_verify`: phantom-symbol check for one file
    /// (`docs/proposal-skylos.md` §3.5). `isError: true` reflects a
    /// lookup failure (no storage, bad args), not a `fail` verdict —
    /// `fail`/`pass` are both `isError: false` results the agent reads
    /// from the leading `✅`/`❌` in the text, the same convention
    /// `weave_policy_lint` already uses for its own lint verdict.
    fn call_verify(&self, args: &Value) -> CallToolResult {
        let Some(file) = args.get("file").and_then(|v| v.as_str()) else {
            return CallToolResult::err("Missing 'file' parameter");
        };
        let range = args.get("range").and_then(|v| v.as_array()).and_then(|a| {
            let start = a.first()?.as_u64()? as u32;
            let end = a.get(1)?.as_u64()? as u32;
            Some((start, end))
        });
        match weave_verify(&*self.storage.borrow(), VerifyArgs { file, range }) {
            Ok(result) => CallToolResult::ok(result.text),
            Err(e) => CallToolResult::err(format!("error: {e}")),
        }
    }

    fn call_repo_map(
        &self,
        args: &Value,
        #[allow(unused_variables)] guard: GuardRef,
    ) -> CallToolResult {
        let max_files = args
            .get("max_files")
            .and_then(|v| v.as_u64())
            .map(|d| d as usize)
            .unwrap_or(50);
        let module = args.get("module").and_then(|v| v.as_bool());
        let max_tokens = args
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .map(|d| d as usize);
        #[cfg(feature = "rbac")]
        let masker = guard.map(|g| move |n: &Node| g.mask_node(n));
        #[cfg(feature = "rbac")]
        let mask: Option<&dyn Fn(&Node) -> Node> =
            masker.as_ref().map(|c| c as &dyn Fn(&Node) -> Node);
        #[cfg(not(feature = "rbac"))]
        let mask: Option<&dyn Fn(&Node) -> Node> = None;
        let res = weave_repo_map(
            &*self.storage.borrow(),
            &self.csr.borrow(),
            RepoMapArgs {
                max_files,
                module,
                max_tokens,
            },
            mask,
        );
        as_tool_result(res.text)
    }

    /// `weave_explore` (P10.2): one budgeted tool composing the four
    /// narrow pull-style ones plus an exact source excerpt. `repo_root`
    /// (for the excerpt) is `weave_dir`'s parent — the same directory
    /// `weave_dir` is a subdirectory of — `None` for an in-memory backend
    /// with no `weave_dir` set at all.
    fn call_explore(
        &self,
        args: &Value,
        #[allow(unused_variables)] guard: GuardRef,
    ) -> CallToolResult {
        let symbol = args.get("symbol").and_then(|v| v.as_str());
        let max_tokens = args
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .map(|d| d as usize);
        let repo_root = self.weave_dir.as_deref().and_then(|d| d.parent());
        #[cfg(feature = "rbac")]
        let masker = guard.map(|g| move |n: &Node| g.mask_node(n));
        #[cfg(feature = "rbac")]
        let mask: Option<&dyn Fn(&Node) -> Node> =
            masker.as_ref().map(|c| c as &dyn Fn(&Node) -> Node);
        #[cfg(not(feature = "rbac"))]
        let mask: Option<&dyn Fn(&Node) -> Node> = None;
        let res = weave_explore(
            &*self.storage.borrow(),
            &self.csr.borrow(),
            repo_root,
            ExploreArgs { symbol, max_tokens },
            mask,
        );
        as_tool_result(res.text)
    }

    /// `weave_find_all` (P10.4, feature `fts`): exhaustive symbol-body
    /// text search — see `find_all.rs`'s own doc comment for why this
    /// rides the existing FTS5 substrate instead of a new regex crate.
    #[cfg(feature = "fts")]
    fn call_find_all(
        &self,
        args: &Value,
        #[allow(unused_variables)] guard: GuardRef,
    ) -> CallToolResult {
        let Some(pattern) = args.get("pattern").and_then(|v| v.as_str()) else {
            return CallToolResult::err("Missing 'pattern' parameter");
        };
        let path = args.get("path").and_then(|v| v.as_str());
        let language = args.get("language").and_then(|v| v.as_str());
        let kind = args.get("kind").and_then(|v| v.as_str());
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|d| d as usize)
            .unwrap_or(50);
        let max_tokens = args
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .map(|d| d as usize);
        #[cfg(feature = "rbac")]
        let masker = guard.map(|g| move |n: &Node| g.mask_node(n));
        #[cfg(feature = "rbac")]
        let mask: Option<&dyn Fn(&Node) -> Node> =
            masker.as_ref().map(|c| c as &dyn Fn(&Node) -> Node);
        #[cfg(not(feature = "rbac"))]
        let mask: Option<&dyn Fn(&Node) -> Node> = None;
        let res = weave_find_all(
            &*self.storage.borrow(),
            FindAllArgs {
                pattern,
                path,
                language,
                kind,
                limit,
                max_tokens,
            },
            mask,
        );
        CallToolResult::ok(res.text)
    }

    fn call_file_api(
        &self,
        args: &Value,
        #[allow(unused_variables)] guard: GuardRef,
    ) -> CallToolResult {
        let paths_vec: Vec<&str> = match args.get("paths").and_then(|v| v.as_array()) {
            Some(arr) => arr.iter().filter_map(|v| v.as_str()).collect(),
            None => vec![],
        };
        let max_tokens = args
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .map(|d| d as usize);
        #[cfg(feature = "rbac")]
        let masker = guard.map(|g| move |n: &Node| g.mask_node(n));
        #[cfg(feature = "rbac")]
        let mask: Option<&dyn Fn(&Node) -> Node> =
            masker.as_ref().map(|c| c as &dyn Fn(&Node) -> Node);
        #[cfg(not(feature = "rbac"))]
        let mask: Option<&dyn Fn(&Node) -> Node> = None;
        let res = weave_file_api(
            &*self.storage.borrow(),
            FileApiArgs {
                paths: &paths_vec,
                max_tokens,
            },
            mask,
        );
        CallToolResult::ok(crate::file_api::render_cards(&res.cards, max_tokens))
    }

    fn call_trace_calls(
        &self,
        args: &Value,
        #[allow(unused_variables)] guard: GuardRef,
    ) -> CallToolResult {
        let symbol = match args.get("symbol").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return CallToolResult::err("Missing 'symbol' parameter"),
        };
        let depth = args
            .get("depth")
            .and_then(|v| v.as_u64())
            .map(|d| d as u32)
            .unwrap_or(2);
        let max_tokens = args
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .map(|d| d as usize);
        let precise_only = args
            .get("precise_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        #[cfg(feature = "rbac")]
        let masker = guard.map(|g| move |n: &Node| g.mask_node(n));
        #[cfg(feature = "rbac")]
        let mask: Option<&dyn Fn(&Node) -> Node> =
            masker.as_ref().map(|c| c as &dyn Fn(&Node) -> Node);
        #[cfg(not(feature = "rbac"))]
        let mask: Option<&dyn Fn(&Node) -> Node> = None;
        let res = weave_trace_calls(
            &*self.storage.borrow(),
            &self.csr.borrow(),
            TraceCallsArgs {
                symbol,
                depth,
                max_tokens,
                precise_only,
            },
            mask,
        );
        as_tool_result(res.text)
    }

    fn call_impact_radius(
        &self,
        args: &Value,
        #[allow(unused_variables)] guard: GuardRef,
    ) -> CallToolResult {
        let symbol = match args.get("symbol").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return CallToolResult::err("Missing 'symbol' parameter"),
        };
        let max_tokens = args
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .map(|d| d as usize);
        #[cfg(feature = "rbac")]
        let masker = guard.map(|g| move |n: &Node| g.mask_node(n));
        #[cfg(feature = "rbac")]
        let mask: Option<&dyn Fn(&Node) -> Node> =
            masker.as_ref().map(|c| c as &dyn Fn(&Node) -> Node);
        #[cfg(not(feature = "rbac"))]
        let mask: Option<&dyn Fn(&Node) -> Node> = None;
        let res = weave_impact_radius(
            &*self.storage.borrow(),
            &self.csr.borrow(),
            ImpactRadiusArgs { symbol, max_tokens },
            mask,
        );
        as_tool_result(res.text)
    }

    #[cfg(feature = "notes")]
    fn call_pin_note(&self, args: &Value) -> CallToolResult {
        let Some(symbol) = args.get("symbol").and_then(|v| v.as_str()) else {
            return CallToolResult::err("Missing 'symbol' parameter");
        };
        let Some(text) = args.get("text").and_then(|v| v.as_str()) else {
            return CallToolResult::err("Missing 'text' parameter");
        };
        let res = weave_pin_note(
            &*self.storage.borrow(),
            Path::new("."),
            PinNoteArgs {
                symbol,
                text,
                tier: args.get("tier").and_then(|v| v.as_str()),
                kind: args.get("kind").and_then(|v| v.as_str()),
            },
        );
        if res.starts_with("error:") {
            CallToolResult::err(res)
        } else {
            CallToolResult::ok(res)
        }
    }

    #[cfg(feature = "vector")]
    fn call_search_semantic(
        &self,
        args: &Value,
        #[allow(unused_variables)] guard: GuardRef,
    ) -> CallToolResult {
        let Some(query) = args.get("query").and_then(|v| v.as_str()) else {
            return CallToolResult::err("Missing 'query' parameter");
        };
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|d| d as usize)
            .unwrap_or(5);
        #[cfg(feature = "rbac")]
        let checker = guard.map(|g| move |n: &Node| g.visible(n));
        #[cfg(feature = "rbac")]
        let visible: Option<&dyn Fn(&Node) -> bool> =
            checker.as_ref().map(|c| c as &dyn Fn(&Node) -> bool);
        #[cfg(not(feature = "rbac"))]
        let visible: Option<&dyn Fn(&Node) -> bool> = None;
        let hits = match crate::search_semantic::weave_search_semantic(
            &*self.storage.borrow(),
            crate::tools::SemanticSearchArgs { query, limit },
            visible,
        ) {
            Ok(hits) => hits,
            Err(e) => return CallToolResult::err(format!("error: {e}")),
        };
        if hits.is_empty() {
            return CallToolResult::ok(format!("No visible semantic matches for \"{query}\"."));
        }
        let text = hits
            .iter()
            .map(|h| {
                let tier = if h.direct { "direct" } else { "metadata" };
                format!(
                    "{} ({}:{}) [{tier}]",
                    h.node.symbol, h.node.path, h.node.line_start
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        CallToolResult::ok(text)
    }

    #[cfg(feature = "policy-lint")]
    fn call_policy_lint(&self, #[allow(unused_variables)] guard: GuardRef) -> CallToolResult {
        let storage = self.storage.borrow();
        let (nodes, edges) = match (storage.all_nodes(), storage.all_edges()) {
            (Ok(n), Ok(e)) => (n, e),
            (Err(e), _) | (_, Err(e)) => return CallToolResult::err(format!("error: {e}")),
        };
        drop(storage);
        // Same one-guard rule as `weave policy lint`: a masked
        // identity's clean run only means "no violations it could see"
        // — never a repo-wide compliance guarantee.
        #[cfg(feature = "rbac")]
        let (nodes, edges) = match guard {
            Some(guard) => {
                let visible_nodes: Vec<Node> =
                    nodes.into_iter().filter(|n| guard.visible(n)).collect();
                let visible_ids: std::collections::HashSet<_> =
                    visible_nodes.iter().map(|n| n.id).collect();
                let visible_edges = edges
                    .into_iter()
                    .filter(|e| {
                        visible_ids.contains(&e.source_id) && visible_ids.contains(&e.target_id)
                    })
                    .collect();
                (visible_nodes, visible_edges)
            }
            None => (nodes, edges),
        };
        let weave_dir = self
            .weave_dir
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from(".weave"));
        let policy_path = weave_dir.join("policy.yaml");
        #[cfg(feature = "rbac")]
        let current_roles: Vec<String> = guard.map(|g| g.roles().to_vec()).unwrap_or_default();
        #[cfg(not(feature = "rbac"))]
        let current_roles: Vec<String> = Vec::new();
        match crate::policy_lint::weave_policy_lint(&policy_path, &nodes, &edges, &current_roles) {
            Ok(text) => CallToolResult::ok(text),
            Err(e) => CallToolResult::err(format!("error: {e}")),
        }
    }
}

#[cfg(test)]
mod tests;
