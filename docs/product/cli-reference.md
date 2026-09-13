# CLI Reference

Every command in Weave Graph is a `clap` subcommand with unified argument parsing. Commands always show up in `weave --help`, even when a specific Cargo feature is not compiled in. If you invoke a command requiring an uncompiled feature, `weave` exits cleanly with a helpful error:

```text
Error: `<command>` requires the `<feature>` feature, which is not compiled into this binary.
Rebuild with `--features <feature>`, or install the prebuilt `weave` / `weave-custom` variant.
```

---

## Tier 1: Base Tier (Deterministic Core — No Features Required)

These commands execute 100% deterministically with zero network calls, zero LLMs, and zero external service dependencies. Available in all builds: 41.1 MB stripped release binary by default (`cargo build --release`, all 29 languages), 9.6 MB with `--no-default-features` (8 core languages only); peak RAM stays under the 80 MB ceiling (measured ~60 MB for 500k symbols).

### `weave init`
Initializes the repository for Weave Graph intelligence, registers the MCP server configuration for AI coding agents, and configures version control / search ignore rules.
```bash
weave init [--mode single|multiple] [--path <dir>]
```
- `--mode single` *(default)*: Configures single-repository indexing in `.weave/config.toml`.
- `--mode multiple`: Scaffolds a multi-repo `[federation]` configuration and CI cache templates.
- `--path <dir>`: Target repository directory *(default: `.`)*.

#### Actions Performed by `weave init`
1. **Scaffolds `.weave/config.toml`**:
   Writes the initial repository configuration specifying runtime mode (`single` or `multiple`), setting up indexing policies and optional federation/storage sections.
2. **Auto-Registers MCP in `.mcp.json`**:
   Provisions or merges into `.mcp.json` at the repository root, adding the Weave MCP server:
   ```json
   {
     "mcpServers": {
       "weave": {
         "command": "weave",
         "args": ["serve", "--mcp"]
       }
     }
   }
   ```
   - **Non-destructive & Merging**: If `.mcp.json` already exists (e.g. configuring `graft`, `filesystem`, or custom servers), existing servers and properties are preserved.
   - **Zero-Config Agent Support**: Supported out of the box by Claude Code, Cursor, Windsurf, Google Antigravity, Gemini Code Assist, GitHub Copilot, Codex, OpenCode, Hermes Agent, and Kiro.
3. **Smart Ignore File Management (`.gitignore` & `.ignore`)**:
   - Ensures runtime databases, locks, and rebuild staging swap files (`.weave/*`) are excluded from Git commits and fast searchers like `ripgrep`, while `.weave/config.toml` remains tracked and committable in version control.
   - Intelligently manages ignore rules:
     - For `.gitignore`: Configures `.weave/*` and `!.weave/config.toml`. If an existing `.gitignore` had a blanket `.weave/` or `.weave`, it is automatically converted so `config.toml` is not suppressed by directory exclusion.
     - For `.ignore`: If present (consulted by `ripgrep` before `.gitignore`), configures `!.weave/`, `.weave/*`, and `!.weave/config.toml` so `ripgrep` searches `config.toml` without scanning binary database files.
     - If neither exists, creates `.gitignore` with `.weave/*` and `!.weave/config.toml` by default.
   - Idempotent: safe to run multiple times without duplicating ignore entries.

### `weave index`
Builds or updates the SQLite code intelligence graph (`.weave/graph.db`) using Tree-sitter parsers across 29 languages.
```bash
weave index [--path <dir>] [--incremental] [--watch]
```
- `--path <dir>`: Root path of the repository to index *(default: `.`)*.
- `--incremental`: Touches only modified files since the last indexing cycle. Automatically falls back to a clean full rebuild above a 10% modified-file ratio (never below 100 modified files) — a fixed default, not a `.weave/config.toml` key.
- `--watch` *(feature: `watch`)*: Runs in the foreground, debouncing file edits and checking impact blast radius before reindexing.

### `weave status`
Summarizes the indexed graph and pending workspace markers.
```bash
weave status [--path <dir>]
```
Displays total file count, symbol count, edge count, database size, and any deferred reindex markers.

### `weave serve`
Launches the Model Context Protocol (MCP) server for AI coding assistants (Claude Code, Cursor, Windsurf, Gemini).
```bash
weave serve --mcp [--transport stdio|http] [--host <addr>] [--port <port>] [--allow-remote] [--require-as]
```
- `--mcp`: Starts the MCP server protocol loop.
- `--transport stdio|http`: Communication channel *(default: `stdio`)*.
- `--host <addr>`: Bind address for HTTP transport *(default: `127.0.0.1`)*.
- `--port <port>`: Port for HTTP transport *(default: `8080`)*.
- `--allow-remote`: Explicit opt-in flag required to bind non-loopback addresses. MCP exposes raw code AST structure and is loopback-restricted by default.
- `--require-as` *(feature: `rbac`)*: Refuses to start the server at all if `--as <subject>` is omitted — for a shared/multi-tenant deployment where an unmasked session by omission is unacceptable. `.weave/config.toml`'s `[rbac] require_identity = true` does the same, repo-wide. Every other RBAC-gated command's own "no `--as` == unmasked" default is unaffected.

### `weave query`
Executes an exact graph traversal expression against the active index.
```bash
weave query "<expression>" [--path <dir>]
```
Supported expression forms:
- `callers(<symbol>)`: Finds direct inbound callers of a function or method.
- `callees(<symbol>)`: Finds direct outbound functions called by this symbol.
- `impact(<symbol>)`: Computes the transitive multi-hop blast radius.
- `path(<source>, <target>)`: Computes the shortest call chain between two symbols.
- `latency(<symbol>)` *(feature: `otel`)*: Displays runtime latency percentiles (p50, p95, p99) and error rate.

### `weave export`
Exports a symbol's N-hop neighborhood as structured JSON.
```bash
weave export --symbol <name> [--depth <n>] [--path <dir>]
```
- `--symbol <name>`: Target symbol to export.
- `--depth <n>`: Traversal depth hops *(default: `2`)*.

### `weave blast`
PR blast-radius analysis comparing branch changes against a base commit.
```bash
weave blast --base <ref> [--format md|json] [--out <file>] [--depth <n>] [--direction callers|callees|both] [--skip --reason <text>] [--path <dir>]
```
- `--base <ref>`: Git reference to diff against using three-dot merge-base (e.g. `main` or `origin/main`).
- `--format md|json`: Output format *(default: `md`)*.
- `--out <file>`: Writes report to a file instead of stdout (ideal for `gh pr comment`).
- `--depth <n>`: Maximum transitive hops to trace from touched symbols *(default: `2`)*.
- `--direction callers|callees|both`: Direction to walk *(default: `callers`)*.
- `--skip`: Waives the blast-radius computation entirely (`impl.md` M3.10) — requires `--reason <text>`; emits a warning banner + a Waiver Notice in the output and exits `0`. `WEAVE_SKIP_BLAST=1` does the same via CI env var (no `--reason` required — the env var itself is the audit trail). With `--features rbac` and a bound `--as <subject>`, the identity must hold the `"allow-drift"` role or the waiver is refused. **Omitting `--as` entirely** is only unrestricted when this repo's `[rbac.users]` config grants `"allow-drift"` to nobody; if it grants that role to anyone, an identity-less waiver is refused outright (`Use --as <subject> to authenticate`) — see `weave check-contracts` below for the same rule.

### `weave report`
Generates architectural summary reports and visualization artifacts.
```bash
weave report [--path <dir>] [--html] [--open]
```
- Produces `WEAVE_REPORT.md` and a native JSON Canvas (`.canvas`) file compatible with Obsidian.
- `--html` *(feature: `viz`)*: Emits a standalone, zero-dependency offline SVG/HTML viewer bundle.
- `--open` *(feature: `viz`)*: Opens the generated HTML bundle directly in the default browser.

### `weave config`
Inspects or modifies scalar configuration keys in `.weave/config.toml`.
```bash
weave config set <key> <value> [--path <dir>]
weave config get <key> [--path <dir>]
```
- Examples: `weave config set storage.home ~/.weave/vault`, `weave config get federation.staleness_policy`.

---

## Tier 2: Team Profile (`--features team` = `docs` + `federation`)

Enables cross-repository composition and Markdown knowledge graphs without network calls or central servers.

### `weave link`
Composes two local repository subgraphs into a federated code graph.
```bash
weave link [repo-a] [repo-b]
```
- Discovers cross-repo dependencies, runs Tarjan's SCC to detect multi-repo cycles, and records cryptographic API contract hashes.
- If only one repository is specified, resolves the second from `[federation] linked_repos` in `.weave/config.toml`.

### `weave query-federated`
Executes queries across the composed multi-repo graph created by `weave link`.
```bash
weave query-federated <repo-a> <repo-b> "<expression>"
```
- Traverses call edges seamlessly across repository boundaries.

### `weave report-federated`
Generates a unified `.canvas` architecture map spanning multiple linked repositories.
```bash
weave report-federated <repo-a> <repo-b> [--out <file>]
```

### `weave check-contracts`
Validates public API contracts between linked repositories in CI pipelines.
```bash
weave check-contracts [--diff] [--scoped] [--allow-drift | --allow-drift-for <repo>] [--warn-only] [--reason <text>] [--path <dir>]
```
- `--diff`: Displays symbol-level added, modified, and removed breakdown upon contract divergence.
- `--scoped`: Enforces failure only for symbols actually imported by the consuming repository, ignoring untouched provider exports.
- Controlled by `[federation] staleness_policy` (`warn`, `strict`, `ignore`).
- `--allow-drift`: Waives drift across every linked repo (`impl.md` M3.10); `--allow-drift-for <repo>` waives just one named peer. Both require `--reason <text>`; a waived repo's drift is still reported but never fails the exit code.
- `--warn-only`: Downgrades a `strict`-policy failure to advisory (never hides *which* repos drifted) — no `--reason` needed, since it doesn't waive anything, just softens the exit code.
- CI env-var equivalents (no `--reason` required — the env var is its own audit trail): `WEAVE_SKIP_CONTRACTS=1` skips the check entirely; `WEAVE_STALENESS_POLICY_OVERRIDE=warn|ignore` overrides the configured policy; `WEAVE_ALLOW_DRIFT_REPOS=repo-a,repo-b` waives specific repos.
- With `--features rbac` and a bound `--as <subject>`, any of the above waivers require the identity to hold the `"allow-drift"` role, or they're refused. **Omitting `--as`** behaves like a non-`rbac` build (unrestricted) *unless* this repo's own `[rbac.users]` config already grants `"allow-drift"` to someone — in that case an anonymous waiver is rejected outright, so a repo that opted into role-gated waivers can't be bypassed by simply dropping `--as`. A repo that never configured `allow-drift` for anyone sees no change.

### `weave plan-migration`
Generates a cross-repo migration plan for a deprecated or modified symbol.
```bash
weave plan-migration --symbol <name> [--path <dir>]
```
- Identifies every file, line, and downstream consumer across all linked repositories requiring updates.

---

## Tier 3: Knowledge & Developer Experience Tier (`notes`, `watch`, `viz`)

Captures developer knowledge, automates background updates, and enhances visualization.

### `weave note pin` & `weave note list` (feature: `notes`)
Pins persistent architectural decisions or debugging notes directly to graph symbols.
```bash
weave note pin [--keep] [--kind <category>] <symbol> <text> [--path <dir>]
weave note list [--path <dir>]
```
- Ephemeral by default (24-hour TTL).
- `--keep`: Crystallizes the note (never expires; tracked via content hash staleness).
- Automatically reattached across reindexing via monikers. If a symbol is deleted, the note is marked as `[orphaned]` rather than deleted.

### `weave index --watch` (feature: `watch`)
Watches the local filesystem for changes, debouncing rapid edits into atomic incremental updates.
```bash
weave index --watch [--path <dir>]
```
- If an edit's transitive blast radius exceeds `blast_radius_ceiling`, the reindex is deferred behind a safety marker until manually triggered.

### `weave viz` (feature: `viz`)
Opens or serves the interactive architectural graph viewer.
```bash
weave viz [--open <bool>] [--port <port>] [--path <dir>]
```
- Operates in static `file://` mode or local HTTP server mode (`[viz] mode = "server"`).

---

## Tier 4: Custom / Self-Hosted Tier (`--features custom`)

Enterprise capabilities for centralized deployment, security boundaries, telemetry, and CI acceleration.

### Global RBAC Identity: `--as <subject>` (feature: `rbac`)
A global flag, threaded into every one of: `query`, `report`, `export`, `search`, `serve --mcp`, `policy lint`, `policy drift`, `blast`, and `check-contracts`:
```bash
weave --as <subject> query "callers(PaymentGateway.charge)"
weave --as <subject> serve --mcp
```
For `query`/`report`/`export`/`search`/`serve --mcp`/`policy lint`/`policy drift`, this enforces query-layer masking based on roles resolved from `.weave/config.toml`'s `[rbac.users]` (overlaid with the SCIM directory). For `blast`/`check-contracts`, `--as` instead gates *waiver permission* (`impl.md` M3.10, see above) — a different use of the same flag: it checks whether the identity holds `"allow-drift"`, not what it can see.

### `weave rbac serve-scim` (feature: `rbac`)
Runs the loopback SCIM 2.0 provisioning endpoint for enterprise IdP synchronization.
```bash
weave rbac serve-scim [--port <port>] [--path <dir>]
```
- `--port <port>` *(default: `9292`)*.
- Generic SCIM 2.0 (`POST /` provision, `DELETE /{subject}` deprovision, `POST /sync` refresh) — any SCIM-capable IdP can push to it; no GitHub-specific or OAuth integration exists.
- Writes identities and roles to `.weave/rbac-directory.toml`.

### `weave policy lint` & `weave policy drift` (feature: `policy-lint`)
Enforces architectural layering rules defined in `.weave/policy.yaml`.
```bash
weave policy lint [--path <dir>]
weave policy drift [--path <dir>]
```
- `lint`: Evaluates `disallow` and `require` boundary rules. Exits non-zero on violations to gate CI builds.
- `drift`: Analyzes structural divergence, detecting architectural dependency cycles and orphaned files.

### `weave traces import` (feature: `otel`)
Ingests OpenTelemetry OTLP JSON trace exports and overlays runtime metrics onto graph nodes.
```bash
weave traces import <file.json> [--path <dir>]
```
- Correlates trace spans with static code symbols.
- Enables latency and error queries via `weave query "latency(<symbol>)"`.

### `weave sync pull` & `weave sync push` (feature: `hub`)
Synchronizes graph snapshots with a centralized `weave-registry` server.
```bash
weave sync pull [--commit <sha>] [--fallback-latest] [--path <dir>]
weave sync push [--signature <sig>] [--path <dir>]
```
- `pull`: Hydrates the exact graph snapshot for a commit via atomic file swap, bypassing cold source parsing in CI runners.
- `push`: Publishes a canonical graph snapshot from trunk branches upon merge. `--signature <sig>` (feature `hub-provenance`): attaches a signature computed by an external signer (e.g. `weave_graph_hub::SnapshotProvenanceVerifier`) — `weave` computes none of its own. The registry only checks it if started with `--provenance-key` (see the [Self-Hosted Guide](self-hosted.md) §6.0); the expected wire format is hex-encoded bytes, and an unconfigured registry accepts any value or none.

### `weave search` (feature: `fts` / `vector`)
Performs hybrid code search combining BM25 full-text indexing and semantic AST embeddings.
```bash
weave search "<query>" [--limit <n>] [--semantic] [--path <dir>]
```
- `--semantic` *(feature: `vector`)*: Activates vector similarity search using `sqlite-vec`.

### `weave ask`, `weave slm`, `weave journal` (feature: `slm`)
Terminal natural-language query routing and local SLM management.
```bash
weave ask "<natural language question>" [--dry-run] [--json]
weave slm pull <model> --sha256 <digest>
weave slm list
weave slm doctor
weave slm review-rules [--confirm <idx>] [--reject <idx>]
weave journal [--since <ref>]
```

---

## Configuration Reference by Tier

| Tier | Config Section | Key Settings |
| :--- | :--- | :--- |
| **Base Core** | `mode` | `mode = "single"` or `"multiple"` |
| | `[storage]` | `home = "/path/to/vault"` (also settable via `WEAVE_HOME`) |
| **Team** | `[federation]` | `linked_repos = ["../repo-b"]`, `staleness_policy = "strict"` (`warn`/`strict`/`ignore`) |
| **Knowledge** | `[watch]` | `enabled = true`, `debounce_ms = 2000`, `blast_radius_ceiling = 200` |
| | `[report]` / `[viz]` | `format = "all"`, `auto_open = false`, `mode = "static"` |
| **Custom / Enterprise** | `[rbac.users]` | `<subject> = ["internal"]` bypasses masking; `["allow-drift"]` grants waiver permission; any other role name is an unprivileged label (see §6 in the Configuration Reference) |
| | `[rbac]` | `require_identity = true` — refuses to start `weave serve --mcp` without `--as` (same as `--require-as`) |
| | `[rbac.scim]` | `token = "<secret>"` — requires a matching `Authorization: Bearer` header on every `weave rbac serve-scim` request |
| | `[hub]` | `url = "http://weave-hub:8080"`, `snapshot_retention = 20`, `token = "<secret>"` (only if the registry was started with `--auth-token`) |
| | `[slm]` | `model = "qwen2.5-coder-0.5b-q4_k_m"` |

`[index]` bailout thresholds are compiled-in defaults, not a config section — see the full Configuration Reference for what's actually read from `.weave/config.toml`.
