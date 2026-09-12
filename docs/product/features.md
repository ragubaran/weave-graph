# Features

Everything beyond the deterministic core is an off-by-default Cargo
feature. Enabling one never changes the meaning of core behavior, and
compiling a feature you don't use costs nothing — no code linked in, no
idle RSS, no latency change on the default paths (measured and enforced
per-feature; see the [Release Notes](release-notes.md#quality-gates-this-release-was-held-to)).

| Feature | Flag | Status |
| :--- | :--- | :--- |
| *(core)* | not required | **Done** — 29-language Tree-sitter indexing, CSR graph, incremental reindex, MCP server, `weave query` |
| [`docs`](#docs) | `--features docs` | **Done** |
| [`federation`](#federation) | `--features federation` | **Done** |
| [`provenance`](#provenance) | `--features provenance` | **Done** |
| [`notes`](#notes) | `--features notes` | **Done** |
| [`watch`](#watch) | `--features watch` | **Done** |
| `weave blast` | not required (base CLI) | **Done** |
| [`viz`](#viz) | `--features viz` | **Done** |
| [`hub`](#hub) | `--features hub` | **Done** (client + `weave-registry` server; provenance trait done, transport wiring pending) |
| [`slm`](#slm) | `--features slm` | **Done** (deterministic scope; real-model latency pending downloaded weights) |
| [`turso`](#turso) | `--features turso` | **Done** (library backend; roadmap in progress) |
| [`python`](#python) | `--features python` | **Done** (separate `pip install` wheel, native binary untouched) |
| MCP live reload | not required (base MCP tier) | **Done** |
| MCP token budgeting | not required (base MCP tier) | **Done** |
| `rbac` | `--features rbac` | **Done** (query-layer masking, M3.0; SCIM directory server, M3.4) |
| `otel` | `--features otel` | **Done** (OTLP JSON trace import + `latency()` query, M3.3) |
| `policy-lint` | `--features policy-lint` | **Done** (YAML boundary lint + drift analytics, M3.2) |
| `fts` | `--features fts` | **Done** (FTS5 + synonym BM25, `weave search`, M3.7 Tier 1) |
| `vector` | `--features vector` | **Not started** (sqlite-vec + quantization, M3.7 Tier 2) |
| `hub-provenance` | `--features hub-provenance` | **Trait boundary done** (M3.6; transport wiring pending) |

Convenience bundles (`weave-graph-cli/Cargo.toml`): `team = [docs, federation]`,
`custom = [team, hub, provenance, rbac, otel, policy-lint, fts, vector]`.

---

## `docs`

Markdown/Obsidian ingestion: `weave index` additionally parses every
Markdown file, extracting wikilinks (`[[Note]]`, `[[Note#Section]]`),
frontmatter `tags`/`aliases`, and backtick code references. Wikilinks that
resolve to another indexed note become `LINKS_TO` edges; a backtick
reference that resolves to an indexed code symbol (e.g. `` `AuthService.verify()` ``)
becomes an `EXPLAINS_RATIONALE` edge — an unresolvable reference produces
no edge, never a dangling one. `.canvas` export (`weave report`) picks
these nodes up automatically — the exported JSON is schema-correct
[JSON Canvas](https://jsoncanvas.org), which Obsidian's Graph View (and
other zero-install canvas viewers) consumes natively. Not independently
verified inside the actual Obsidian application in this repo's own test
environment; the schema conformance is tested, opening the file in
Obsidian itself is not.

## `federation`

Composes two or more already-indexed repos' subgraphs **locally, with no
network call** — `weave link repo-a repo-b`. Detects cross-repo dependency
cycles (Tarjan's SCC over the composed graph) and, alongside
`provenance`/contract-hashing, records each repo's exported-API contract
hash as the other's expectation for `weave check-contracts` to enforce in
CI. See [CLI Reference](cli-reference.md#weave-link-feature-federation) and
[Getting Started](getting-started.md#multiple-mode-several-local-repos-no-hosted-service).

## `provenance`

A `ProvenanceProvider` trait boundary for Merkle-signed note/link
provenance — `attach(doc_id, commit_hash)` / `verify(...)`. `weave` ships a
`MockProvenanceProvider` and renders a `## Document Provenance` section in
`weave report` / a `doc_provenance` field in `weave export` whenever signed
links exist; it never attaches provenance itself. A real provider (e.g. an
external signing service) is wired by a host application against this
trait — no specific provider is a hard dependency of `weave`.

## `notes`

Pin structured knowledge onto a graph symbol — an architecture decision, a
test-failure diagnosis, a "why this exists" rationale — so a later session
(human or AI agent) sees it without re-deriving it, via plain SQL, never an
embedding/retrieval layer.

```bash
weave note pin AuthService.verify "Rate-limited per RFC 6238 §3; do not remove the jitter."
weave note pin --keep --kind arch_decision PaymentQueue "Single-writer by design — see ADR-014."
weave note list
```

Two tiers: **ephemeral** (default, 24h TTL — a session scratchpad) and
**crystallized** (`--keep`, never expires, gets content-hash staleness
tracking: if the symbol's source changes non-trivially, `weave note list`
flags the note stale). Notes reattach by moniker across a reindex — a
purged-and-reinserted symbol keeps its notes; a genuinely deleted symbol's
notes are reported with an `orphaned` tag, never silently dropped. The MCP pair
`weave_pin_note` / `weave_recall_notes` exposes the same capability to AI
agents — see [MCP Integration](mcp-integration.md).

## `watch`

Automatic incremental reindexing on file save, with a blast-radius safety
gate. Two entry points share one engine: `weave index --watch` (foreground)
and a background thread inside `weave serve --mcp` (gated on
`[watch] enabled = true`).

A burst of file-change events debounces into exactly one reindex attempt
(`[watch] debounce_ms`, default 2000ms). Before reindexing, `weave` computes
the union of every changed file's transitive impact radius; below
`[watch] blast_radius_ceiling` (default 200 symbols) it auto-reindexes,
at or above it the change is **deferred** behind a visible marker instead —
`weave status` prints it, and every MCP tool response carries a warning
naming the pending files and blast-radius size, until you run
`weave index` manually. A reverted change that drops back under the
ceiling resumes auto-sync on its own. Files still inside the debounce
window (changed but not yet reindexed) are surfaced separately, both in
`weave status` and in MCP responses, so an agent can tell "still settling"
apart from "deferred, needs a manual index."

## `weave blast` (no feature flag — base CLI)

PR blast-radius comment mode: `weave blast --base main` diffs the current
branch against `main` (three-dot/merge-base, so it never blames a PR for
commits `main` picked up after the branch point), then renders a
markdown or JSON report of every symbol the diff's changed files touch
transitively — folded by architectural module once the touched-symbol
count crosses the same ~200-node budget `weave report` uses. `weave`
itself never talks to GitHub; pipe the output into `gh pr comment` from
CI. Requires `fetch-depth: 0` in the CI checkout (a shallow clone fails
with a message naming the fix, not a confusing raw `git` error).

## `viz`

An offline HTML viewer for `weave report`'s output — no server dependency
by default. `weave report --html` renders a standalone bundle (canvas JSON
embedded, rendered as SVG via vanilla JS) alongside the existing Markdown +
`.canvas` files; `weave viz` re-opens it, or (`[viz] mode = "server"`)
serves it from a loopback-only (`127.0.0.1`) static file server with
path-traversal refused.

## `hub`

Centralized snapshot registry and client sync: `weave sync pull` hydrates the
graph for a commit (falling back to the hub's `latest` snapshot on request),
`weave sync push` publishes the current snapshot (refuses off the default branch,
retries once on a `409` conflict). Includes both the zero-dependency HTTP client
and the lightweight `weave-registry` standalone server.

### Hub Ecosystem & Provenance Status
- **Lodestone Nexus Provenance (`hub-provenance`)**:
  `SnapshotProvenanceVerifier` trait and reference implementation are done and
  tested standalone.
- **Transport Wiring**:
  Not yet wired into `weave sync push/pull` — needs a registry-side storage
  schema change (signature sidecar) to actually transmit it. Stated honestly,
  not silently skipped.
- **Unstarted Ecosystem Extensions**:
  `hub-canvas`, `hub-webhooks`, and chunked upload: not started this pass.

## `slm`

The `slm` feature integrates **Small Language Models (SLMs)** and links **Frontier LLMs** with Weave Graph through a structured 3-tier architecture.

### 1. The 3-Tier Intelligence Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│ Tier 1: Zero-LLM Deterministic Core (<2ms, 100% Offline, <80MB RAM)     │
│   Tree-sitter AST parsers • CSR adjacency matrices • SQLite WAL store   │
└────────────────────────────────────┬────────────────────────────────────┘
                                     │
         ┌───────────────────────────┴───────────────────────────┐
         ▼                                                       ▼
┌─────────────────────────────────┐   ┌───────────────────────────────────┐
│ Tier 2: Local SLM (0.5B–3B)     │   │ Tier 3: Frontier LLM Agents       │
│ • Terminal NL Router (weave ask)│   │ • Claude Code, Cursor, Windsurf   │
│ • Grounded parameter resolution │   │ • Native MCP Protocol (stdio/SSE) │
│ • ADR rule extraction           │   │ • 92% context token reduction     │
│ • Zero cloud data egress        │   │ • Subgraph & blast radius pruning │
└─────────────────────────────────┘   └───────────────────────────────────┘
```

### 2. Local SLM Linking (`weave ask` & `weave slm`)

A natural-language query router **for a human at a terminal** (`weave ask`), explicitly never on the MCP/agent path (agents already emit exact tool calls — an intermediate translation layer would only add latency and lose fidelity).

- **Strict AST Parameter Grounding**: Translates natural language intents (e.g., *"Which services call JWT verification?"*) into exact graph traversals (`callers("verifyJWT", depth=2)`). Every symbol is resolved against the live symbol table before dispatch; unresolvable or ambiguous tokens immediately report a routing failure rather than hallucinating.
- **Model Lifecycle & Management**:
  - `weave slm pull <model> --sha256 <digest>`: Downloads and checksum-verifies compact GGUF models (e.g., `qwen2.5-coder-0.5b`, `llama-3.2-1b`) into `$XDG_CACHE_HOME/weave/models/`.
  - `weave slm list`: Displays registered models, quantization levels (Q4_K_M), and local cache availability.
  - `weave slm doctor`: Executes a built-in validation suite against held-out prompts, asserting tool-selection accuracy, symbol-grounding rate, and Time-to-First-Token (TTFT).
- **Zero Idle Overhead**: Falls back to an instant deterministic heuristic router when no local model is pulled. Model weights load lazily — compiling with `--features slm` incurs **0 MB idle RSS** until `weave ask` is invoked.

### 3. Frontier LLM Linking via MCP Server

Weave Graph links frontier coding models (Claude 3.7 Sonnet, GPT-4o, o3, Gemini Pro) via standard Model Context Protocol:

- **Subgraph Extraction over File Dumps**: Traditional workflows dump 50,000–100,000 raw source tokens into the LLM context window, causing prompt cost blowouts and attention dilution ("lost in the middle").
- **Targeted MCP Graph Slices**: Weave Graph provides focused MCP tools (`callers`, `callees`, `impact`, `path`), returning exact 1,000-token subgraphs with transitive dependencies in $<2\text{ms}$ — slashing LLM context consumption by over **92%**.
- **Synergistic Workflow**: Terminal developers use Tier 2 Local SLM (`weave ask`) for zero-cloud triage, while autonomous IDE agents leverage Tier 3 MCP tools for high-precision refactoring.

### 4. Knowledge & Document Linking Commands

- `weave slm review-rules`: Scans Markdown design docs and ADRs for obligation-shaped sentences ("must", "must not", "should never") and extracts candidate architectural invariants into `.weave/rules.toml` for human confirmation (`--confirm`/`--reject`), bridging human prose with automated CI policy checks.
- `weave journal [--since <ref>]`: Combines a Git diff against `<ref>` with the graph delta (touched symbols, callers, and blast radius) to synthesize structured changelog summaries with zero model inference overhead.

## `turso`

An alternate `Storage` backend on embedded libSQL, implementing the exact
same trait as the default `rusqlite` backend (same schema, same
migrations, same transaction discipline).

- **Normal Mode Alternative (`--features turso` / `vX.Y.Z-turso`)**:
  Turso is **only available for normal mode** (no vector support), replacing
  bundled SQLite with embedded libSQL ($11.5\text{MB}$ stripped release).
- **SQLite Exclusivity for Default and Vector Modes**:
  1. *Default Normal Mode (`vX.Y.Z`)* uses SQLite exclusively to keep the binary single-engine, ultra-small ($<6.8\text{MB}$), and zero-network.
  2. *Vector Mode (`vX.Y.Z-vector`)* is SQLite-exclusive because Turso does not natively support `sqlite-vec` virtual tables yet.

## `python`

PyO3 bindings (`weave-graph-python`, `pip install`-able as a `maturin`-built
wheel) exposing the same query surface the CLI uses —
`get_node`/`get_edges`/`query_path`/`impact_radius`/`trace_calls` — to a
Python script. The wheel is a **genuinely separate build artifact**: the
native `weave` binary has zero dependency on this crate, and compiling it
into the workspace doesn't change the native binary's size or behavior at
all.

Build the wheel with [`maturin`](https://www.maturin.rs/) — **not** plain
`cargo build`:

```bash
pip install maturin
cd crates/weave-graph-python
maturin build --release --features python
pip install ../../target/wheels/weave_graph-*.whl
```

`cargo build -p weave-graph-python` (or `cargo test` on it) will **fail to
link** with undefined `_Py*` symbol errors, regardless of which Python
interpreter is on your `PATH` or whether `PYO3_PYTHON` is set — this is
expected, not a misconfiguration. The crate's `rlib` target (needed for a
plain `cargo build`) genuinely requires linking against a real
`libpython`; the wheel's `extension-module` build defers that linking to
the host interpreter at import time instead, which only `maturin build`
sets up correctly. Always go through `maturin`.

```python
import weave_graph
g = weave_graph.WeaveGraph(".weave/graph.db")
print(g.impact_radius("AuthService.verify"))
```

## MCP live reload (base MCP tier, no feature flag)

`weave serve --mcp` detects when a *different* process (a manual
`weave index`, or the `watch` background thread) has rewritten the active
database out from under it, and transparently closes and reopens its
connection before answering the next request — bounded to a cheap `stat()`
check roughly every 500ms so the fast path's latency floor is untouched
between reindexes. Without this, a long-lived MCP session would keep
answering from an increasingly stale snapshot indefinitely.

## MCP token budgeting (base MCP tier, no feature flag)

All four core MCP tools (`weave_repo_map`, `weave_file_api`,
`weave_trace_calls`, `weave_impact_radius`) accept an optional
`max_tokens` parameter. When the full-detail response would exceed it,
each tool sheds to a coarser tier — module summary → file boundaries →
symbol-level detail — instead of returning an unbounded blob (e.g. a
hub symbol's full impact radius on a large repo). Omitting `max_tokens`
returns exactly today's behavior; no existing caller needs to change. See
[MCP Integration](mcp-integration.md) for the full tool list and schemas.
