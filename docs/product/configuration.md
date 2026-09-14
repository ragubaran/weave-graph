# Configuration Reference

All configuration for Weave Graph lives in `<repo>/.weave/config.toml`, created automatically by `weave init`.
Every section other than `mode` is optional — an absent section simply disables that capability without pending setup steps. Feature-gated sections apply only when their corresponding Cargo feature is compiled into the binary.

```toml
# Workspace operating mode: "single" | "multiple"
mode = "single"

# ==============================================================================
# Part I: Core & Developer Configuration
# ==============================================================================
[storage]
home = ""                           # Optional central storage directory (default: <repo>/.weave/)
                                     # Also the only supported opt-in past the network-filesystem
                                     # refusal below (alongside the WEAVE_HOME env var) — there is
                                     # no separate "force it anyway" flag.

[federation]                        # feature: federation (team profile)
linked_repos = ["../sibling-repo"]  # Relative paths to locally federated repositories
staleness_policy = "strict"         # Policy for check-contracts: "warn" | "strict" | "ignore"

[watch]                             # feature: watch
enabled = false                     # File watcher background thread: "true" to activate
debounce_ms = 2000                  # Coalesce file modifications into single batch [100, 60000]
blast_radius_ceiling = 200          # Maximum changed symbols allowed before deferring reindex

[report]
format = "canvas"                   # "canvas" (default) | "html" | "all"
auto_open = false                   # Automatically launch browser on report generation

[viz]                               # feature: viz
mode = "static"                     # "static" (file:// bundle) | "server" (loopback HTTP server)

# ==============================================================================
# Part II: Custom Mode / Self-Hosted Enterprise Configuration (feature: custom)
# ==============================================================================
[rbac]                               # feature: rbac
require_identity = false            # true: `weave serve --mcp` refuses to start without `--as`
                                     # (same effect as the `--require-as` CLI flag). Does not
                                     # change query/report/export's own masking default.

[rbac.users]                        # feature: rbac
alice = ["internal"]                # "internal" is the one role name RBAC treats specially:
                                     # bypasses masking entirely. Every other role name below gets
                                     # identical behavior — masked down to pub-visible symbols only.
bob = ["contractor"]                # A role name with no special meaning of its own (see §6).
service_account = ["allow-drift"]   # The other special role: permission to waive
                                     # check-contracts/blast gates (impl.md M3.10, see §7). Once
                                     # granted to anyone, an identity-less waiver (`--as` omitted)
                                     # is refused outright — see §7.

[rbac.scim]                         # feature: rbac
token = ""                          # Optional: require `Authorization: Bearer <token>` on every
                                     # `weave rbac serve-scim` request. Empty/absent: unauthenticated

[rbac.github_roles]                  # feature: github-auth (optional)
# octocat = ["internal"]             # roles for a GitHub login resolved from WEAVE_GITHUB_TOKEN
[rbac.github_org_roles]              # optional organization-to-role mapping
# platform = "internal"              # role granted when GitHub reports membership
[rbac.github_team_roles]             # optional team-to-role mapping (`org/slug`)
# platform/security = "internal"
                                     # (loopback-only trust), unchanged from before this key existed.

[hub]
url = "http://weave-registry:8080"  # URL of centralized weave-registry server
snapshot_retention = 20             # Maximum snapshots retained per repository on hub
token = ""                          # Required only if weave-registry was started with --auth-token;
                                     # value must match exactly.

[slm]
model = "qwen2.5-coder-0.5b-q4_k_m" # Model identifier in $XDG_CACHE_HOME/weave/models/
```

`weave` also reads a `[federation.linked_repos]`-adjacent `weave check-contracts --allow-drift-for <repo>`
CLI flag and matching `WEAVE_*` environment variables for temporary CI waivers — see §7, not this table
(they're not `.weave/config.toml` keys at all).


---

# Part I: Core & Developer Configuration

## 1. Operating Mode (`mode`)

Defines the repository workspace topology:
- `"single"` *(default)*: Optimized for a standalone repository.
- `"multiple"`: Scaffolds multi-repo federation support, cross-repository contract expectations, and CI cache templates.

---

## 2. Storage & Environmental Overrides (`[storage]`)

### Centralized Storage Vault (`WEAVE_HOME`)
By default, Weave Graph stores its SQLite database at `<repo>/.weave/graph.db`. To isolate graph databases outside of the source tree (such as in compliance environments, shared CI agents, or centralized knowledge vaults), define `[storage] home` in config or export the `WEAVE_HOME` environment variable:

```bash
export WEAVE_HOME=~/.weave/vault
```


When `WEAVE_HOME` is active, Weave Graph isolates databases under a deterministic sanitized directory name:
`$WEAVE_HOME/<sanitized-repo-path>/graph.db`.
Indexing operations only touch the active repository's database and swap files.

### Network Filesystem Protection
SQLite Write-Ahead Logging (WAL) requires shared-memory primitives (`shm`) that network mounts (NFS, SMB, CIFS) do not reliably provide. If Weave Graph detects that `.weave/` resides on a network filesystem, it logs a diagnostic warning and refuses to run, to prevent database corruption.

There is no dedicated toggle for this — the only two ways past the refusal are the two storage-relocation mechanisms already documented above (`[storage] home` or `WEAVE_HOME`), which move the database off the network mount entirely rather than reconfiguring how it's accessed there. Once relocated, the check never re-runs against the new location.

---

## 3. Indexing & Rebuild Thresholds

`weave index --incremental` re-parses only modified files. When large refactorings or upstream branch merges occur, executing thousands of localized deletes and re-inserts is slower than a clean rebuild — `weave-graph-core`'s `ReindexConfig` bails out to a full rebuild above a 10% modified-file ratio (never below 100 modified files regardless of ratio). **These thresholds are compiled-in defaults, not `.weave/config.toml` keys** — there is no `[index]` section; `weave config set index.bailout_ratio ...` would write a value nothing reads back. If you need this tunable, that's a real gap to file, not a documented feature today.

---

## 4. Multi-Repo Federation (`[federation]`)

Used by `weave link`, `weave query-federated`, and `weave check-contracts` (requires `--features team` or `federation`):
- `linked_repos` *(array of strings)*: Relative directory paths to partner repositories in the federation.
- `staleness_policy` *(string, default: `"warn"`)*: Controls CI gate behavior during `weave check-contracts`:
  - `"strict"`: Exits with a non-zero code on any contract hash mismatch (recommended for CI).
  - `"warn"`: Prints a diagnostic drift summary but exits with code `0`.
  - `"ignore"`: Skips contract validation.

---

## 5. Developer Experience & Visualization (`[watch]`, `[report]`, `[viz]`)

### Background File Watcher (`[watch]`)
- `enabled` *(boolean, default: `false`)*: Activates the background file watcher thread inside `weave serve --mcp` or `weave index --watch`.
- `debounce_ms` *(integer, default: `2000`)*: Coalescing window in milliseconds to batch rapid file save events into a single reindex cycle. Clamped to `[100, 60000]`.
- `blast_radius_ceiling` *(integer, default: `200`)*: Maximum impacted symbols permitted for automatic reindexing. Edits exceeding this ceiling are deferred behind a pending marker in `weave status` to avoid freezing local systems during massive changes.

### Reporting & Visualization (`[report]`, `[viz]`)
- `[report] format`: Output format (`"canvas"`, `"html"`, or `"all"`). Markdown and JSON Canvas (`.canvas`) are always generated regardless.
- `[report] auto_open`: Automatically launches the default web browser after generating an HTML report.
- `[viz] mode`: Selects between `"static"` (standalone `file://` SVG viewer bundle) and `"server"` (loopback HTTP server on `127.0.0.1`).

---

# Part II: Custom Mode / Self-Hosted Enterprise Configuration

The sections below apply when Weave Graph is compiled with `--features custom` (or specific individual enterprise features). These options configure centralized security, compliance policies, runtime telemetry, and hub synchronization.

---

## 6. Role-Based Access Control (`[rbac.users]`)

Weave Graph enforces security masking directly at the **query layer** across the CLI, Markdown/Canvas reports, exports, and MCP tools (Core Invariant 7).

Masking engages purely based on whether the global `--as <subject>` flag is passed on a given command; there is no `enabled`/`anonymous_role` toggle for masking itself. An identity-less call (`--as` omitted) always resolves to the built-in anonymous identity (zero roles), not a configurable fallback.

The one `[rbac]`-table key that does exist governs a narrower, separate question:
```toml
[rbac]
require_identity = true   # weave serve --mcp refuses to start at all without --as
```
This is for a shared or multi-tenant deployment (a CI runner, a proxied MCP endpoint) where an unmasked session by omission is unacceptable — it does not change `weave query`/`weave report`/`weave export`'s own masking default, only whether the MCP server process is willing to start unbound. `--require-as` on the `weave serve --mcp` command line does the same thing without a repo-wide config change.

### User & Role Mappings (`[rbac.users]`)
Maps individual identities to role-name arrays:
```toml
[rbac.users]
alice = ["internal"]
bob = ["engineer"]
contractor_vendor = ["contractor"]
ci_auditor = ["auditor"]
release_bot = ["allow-drift"]
```

**Only two role names carry any special meaning anywhere in the code** — every other role name (`"engineer"`, `"contractor"`, `"auditor"`, or anything else you make up) is purely a label for your own bookkeeping/audit trail and has **identical** masking behavior:
- `"internal"` — bypasses query-layer masking entirely; the identity sees the full graph.
- `"allow-drift"` — grants permission to invoke a `weave check-contracts`/`weave blast` waiver (`--allow-drift`, `--allow-drift-for`, `--skip`, or their `WEAVE_*` env-var equivalents — see the CLI reference). Independent of `"internal"`: an identity that sees everything isn't automatically trusted to bypass a CI gate. Once this role is granted to *anyone* in `[rbac.users]`, an identity-less waiver (`--as` omitted) is rejected outright — a repo that never grants it sees no change in that behavior.

Everyone else — any other role, multiple roles, or no roles at all — gets the exact same masked view: only symbols the language's own visibility convention marks public (`pub fn` in Rust, `export` in TS/JS, etc.), the same heuristic `weave check-contracts`'s contract hash already uses. There is no per-role custom masking pattern, no `mask = [...]` table, nothing module-scoped to a specific role.

### SCIM 2.0 Identity Directory (`.weave/rbac-directory.toml`)
`weave rbac serve-scim --port 9292` runs a loopback-only SCIM 2.0 endpoint (`POST /Users` to provision, `DELETE /{subject}` to deprovision, `POST /sync` to refresh) that an IdP pushes subject→role assignments to; results land in `.weave/rbac-directory.toml`, same `subject -> [roles]` shape as `[rbac.users]`. It has no `/Groups` endpoint and no vendor-specific integration — it's generic SCIM, and the IdP is responsible for deciding which roles to push for which subject.

Bearer-token authentication is optional (`[rbac.scim] token = "<secret>"` above) — set it and every request needs a matching `Authorization: Bearer <secret>` header or the server rejects it with `401`; leave it unset and the server accepts any local caller (loopback-only trust, same model the base MCP server uses). Worth setting on any shared host, since provisioning can elevate a subject to `"internal"`.
- **Precedence Rule**: Directory records in `rbac-directory.toml` override static entries in `[rbac.users]` for the same subject.
- **Identity Evaluation**: The global `--as <identity>` flag evaluates both static config and SCIM directory records to resolve active roles.

---

## 7. Architectural Policy Linting (`.weave/policy.yaml`)

Declarative architectural boundary enforcement and drift detection. The rules file path (`.weave/policy.yaml`) and CI-fail-on-violation behavior are both fixed, not configurable — there is no `[policy]` config table (no `config_file`/`exit_on_violation` keys); `weave policy lint` always reads `.weave/policy.yaml` and always exits non-zero on any violation.

### Boundary Rule Format (`.weave/policy.yaml`)
Boundary rules define allowable communication paths across architectural modules:
```yaml
rules:
  # Disallow direct database calls from UI or HTTP controllers
  - disallow:
      from: "src/controllers"
      to: "src/db"

  # Require services to mediate billing operations
  - require:
      from: "src/billing"
      to: "src/services/billing_service.rs"
```

---

## 8. Distributed Traces & Telemetry

Connects static AST call graphs with runtime performance data imported via OpenTelemetry OTLP JSON trace exports. There is no `[otel]` config table — `weave traces import <file>` takes the trace file as a required CLI argument (no config-file fallback), and the reported percentiles are fixed at p50/p95/p99 (not a configurable list).

### Trace Matching Behavior
- Correlates span `code.function` or span names with static graph symbols.
- Enables `weave query "latency(<symbol>)"` to report span/error counts and p50/p95/p99 latency (in microseconds) directly in terminal queries and MCP responses.

---

## 9. Centralized Snapshot Registry & Hub (`[hub]`)

Configures client synchronization with a self-hosted `weave-registry` server daemon:

```toml
[hub]
url = "http://weave-registry.internal.corp:8080"
snapshot_retention = 20
token = ""
```

### Settings
- `url` *(string, optional)*: The HTTP endpoint of the centralized registry. Leaving this unset is a fully supported permanent state for offline and local teams.
- `snapshot_retention` *(integer, default: `20`)*: Hint header sent during `weave sync push` specifying how many historical snapshots to retain per repository branch on the hub server.
- `token` *(string, optional)*: Sent as `Authorization: Bearer <token>` on every request. Required only if the registry was started with `weave-registry --auth-token`; a registry started without one accepts requests with or without this key set.
- **Atomic Hydration**: `weave sync pull` downloads canonical graph snapshots and applies them via atomic file swap (`.rebuild`), so a CI runner starts from a hydrated graph instead of a cold source-tree parse.
- **Snapshot signatures** (`weave sync push --signature <hex>`, feature `hub-provenance`): a CLI flag, not a config key — `weave` computes no signature of its own. The registry only verifies it when started with `--provenance-key`; see the [Self-Hosted Guide](self-hosted.md) §6.0.

---

## 10. Local SLM & Natural Language Routing (`[slm]`)

Configures natural-language query routing for human developers at terminals:

```toml
[slm]
model = "qwen2.5-coder-0.5b-q4_k_m" # Model name in $XDG_CACHE_HOME/weave/models/
```

### Settings
- `model`: Identifies the local GGUF model managed via `weave slm pull` and `weave slm list`.
- Lazy loading (no model weights loaded into memory until `weave ask` is explicitly run) is unconditional code behavior, not a config toggle — there is no `lazy_load` key.
- *Note: `slm` is exclusively for terminal human queries and is never invoked on the MCP agent path.*

---

## 11. Hybrid Search & Vector Embeddings (`[fts]`, `[vector]`)

Configures lexical and semantic code search:
- `fts` *(BM25)*: Automatically indexes symbol names, doc comments, signatures, and file paths into SQLite FTS5 with AST-aware synonym expansion.
- `vector`: Utilizes `sqlite-vec` virtual tables for semantic similarity lookups across AST definition chunks via `weave search "<query>" --semantic`.

---

# Part III: Turso Storage Engine (Library-only)

> [!WARNING]
> **Corrected (2026-09-13)**: earlier revisions of this section described a "dedicated `weave-turso` binary variant" that builds separately from the default `weave` binary and safely avoids a C-symbol conflict by construction. No such binary exists anywhere in this repository — `grep -r "weave-turso"` across the workspace turns up nothing but this doc. There is exactly one CLI binary target (`weave`, `crates/weave-graph-cli`), and `--features turso` only adds `weave-graph-store-turso` as an *additional* optional dependency to that same binary — it does not replace or exclude `weave-graph-store-sqlite`, which is a plain, non-optional dependency of `weave-graph-cli` regardless of any feature flag. The paragraphs below describe what's actually true today, not what a future build could be.

## 12. Current support boundary

`TursoStorage` (`crates/weave-graph-store-turso`) is a complete `Storage` trait implementation — same transactional guarantees, same schema migrations as `SqliteStorage` — built on embedded **libSQL** (`libsql = "0.9"`) instead of `rusqlite`. It is real, tested code, benchmarked in its own `benches/turso_latency.rs`.

The supported `weave` CLI uses SQLite. The repository contains a separately
tested `TursoStorage` library implementation, but it is not wired into CLI or
MCP storage selection. No Turso backend selector or Turso distribution is
currently supported; do not configure `storage.backend = "turso"`.

`weave-graph-cli` never constructs a `TursoStorage` today. Every CLI command
opens `SqliteStorage`; `--features turso` only compiles the library dependency:
- There is no `[storage.turso]` config table, no `sync_url`/`auth_token`/replication config, and no CLI flag to select a backend.
- A future selector requires a deliberate build/distribution design and its
  own compatibility, performance, and recovery tests; it is not a runtime
  configuration claim today.

### What's Real Today
`TursoStorage::open(path)` / `open_in_memory()` are usable as a library from
other Rust code. There is nothing in `.weave/config.toml` or the `weave` CLI
that selects it yet.

---

# Part IV: CLI Helpers & Programmatic Configuration

Inspect and update configuration values programmatically:

```bash
# Read a scalar key
weave config get storage.home
weave config get federation.staleness_policy

# Update a scalar key
weave config set federation.staleness_policy strict
weave config set watch.blast_radius_ceiling 300
```

> [!NOTE]
> `weave config set` only writes scalar values (strings, booleans, numbers). Array keys such as `[federation] linked_repos` or user tables such as `[rbac.users]` require editing `.weave/config.toml` directly.
