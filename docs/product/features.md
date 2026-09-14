# Features

Everything beyond the deterministic core is an off-by-default Cargo feature.
The pages below describe implemented interfaces; package, memory, latency,
and semantic-quality targets remain subject to the gates in the internal audit.

| Tier / Profile | Feature | Flag | Description |
| :--- | :--- | :--- | :--- |
| **Base Tier (Core)** | Core Engine | `--no-default-features` | Core-language Tree-sitter indexing, CSR graph, incremental reindex, loopback MCP server, `weave query`, `weave blast` |
| **Team Profile** | [`docs`](#docs) | `--features docs` | Markdown/Obsidian ingestion, wikilinks, backtick code rationales, JSON Canvas export |
| | [`federation`](#federation) | `--features federation` | Multi-repo graph composition, cross-repo cycles, contract hashing & CI verification |
| **Knowledge & DX Tier** | [`notes`](#notes) | `--features notes` | Pinned symbol notes, ephemeral (24h TTL) and crystallized tiers, moniker reattachment |
| | [`watch`](#watch) | `--features watch` | Auto-sync file watcher, debounce queue, blast-radius safety ceiling |
| | [`viz`](#viz) | `--features viz` | Offline standalone HTML viewer, loopback static report server |
| **Custom / Self-Hosted Tier** | [`rbac`](#rbac) | `--features rbac` | Query-layer role-based masking, SCIM 2.0 provisioning server, IdP directory sync |
| **GitHub token identity (optional)** | [`github-auth`](#github-token-identity) | `--features github-auth` | GitHub API identity lookup from `WEAVE_GITHUB_TOKEN` |
| | [`policy-lint`](#policy-lint) | `--features policy-lint` | YAML architectural boundaries, dependency linting, architectural drift analytics |
| | [`otel`](#otel) | `--features otel` | OTLP JSON trace import, node-level latency percentiles and error metrics |
| | [`hub`](#hub) | `--features hub` | Centralized snapshot registry, `weave sync pull/push`, delta sync, CI hydration |
| | [`fts`](#fts) | `--features fts` | BM25 full-text symbol search with AST synonym expansion (`weave search`) |
| | [`vector`](#vector) | `--features vector` | Vector embeddings with `sqlite-vec` virtual tables for semantic symbol retrieval |
| | [`slm`](#slm) | `--features slm` | Natural-language terminal query router (`weave ask`), model management, ADR review |
| | [`provenance`](#provenance) | `--features provenance` | Optional note and document provenance primitives |
| **Extensibility & Runtimes** | [`turso`](#turso) | Library feature only | Embedded libSQL `Storage` implementation; not available through `weave` commands |
| | [`python`](#python) | `--features python` | PyO3 Python bindings wheel (`weave-graph-python`) for offline graph analytics |

### Feature Profiles (Cargo Bundles)
- **Core artifact**: A prior local `--no-default-features` build measured about 9.6 MiB. The <15 MB target applies only to that artifact; the complete 500k-symbol indexing RSS gate remains open.
- **Team Profile (`--features team`)**: `docs` + `federation`. Multi-repo linking, contract checking, and Markdown knowledge integration.
- **Custom Mode / Self-Hosted Profile (`--features custom`)**: Enables the compiled enterprise feature set. Review each capability's authentication and verification status before deployment; this profile is not a certification of every enterprise control.


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
other zero-install canvas viewers) consumes natively. The repository's
schema test passes, but desktop rendering is still unverified: Obsidian's
bundle is present on the verification host, yet macOS cannot launch it
(`kLSNoExecutableErr`, incomplete application bundle).

## `federation`

Composes two or more already-indexed repos' subgraphs **locally, with no
network call** — `weave link repo-a repo-b`. Detects cross-repo dependency
cycles (Tarjan's SCC over the composed graph) and, alongside
`provenance`/contract-hashing, records each repo's exported-API contract
hash as the other's expectation for `weave check-contracts` to enforce in
CI. See [CLI Reference](cli-reference.md#weave-link-feature-federation) and
[Getting Started](getting-started.md#multiple-mode-several-local-repos-no-hosted-service).

## `provenance`

An optional `ProvenanceProvider` trait boundary for note/link provenance
(`attach(doc_id, commit_hash)` / `verify(...)`). `weave` ships a
`MockProvenanceProvider` and renders a `## Document Provenance` section in
`weave report` / a `doc_provenance` field in `weave export` whenever signed
links exist; it never attaches provenance itself. Core indexing, querying,
and MCP operation do not depend on any provenance service. If provenance is
enabled, Merkle/PKI signing is supplied by an external application such as
Lodestone Nexus (or another deployment-supplied provider) wired against this
trait; no signer or PKI implementation is a hard dependency of `weave`.

## `github-auth`

When enabled with `--features github-auth`, commands resolve
`WEAVE_GITHUB_TOKEN` through GitHub's authenticated-user API and use the
returned login and stable user ID as the query identity. Invalid, missing,
or unreachable tokens fail closed to the normal anonymous/public view. The
token is never persisted or emitted in errors or MCP responses. This is a
GitHub token identity lookup only; it is not generic OAuth/OIDC, SAML, or
interactive SSO, and it requires outbound HTTPS access to GitHub.

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
and `weave sync push` publishes the current snapshot (refuses off the default branch,
retries once on a `409` conflict). Includes both the zero-dependency HTTP client
and the lightweight `weave-registry` standalone server daemon.

- **Fast CI Hydration**: Replaces cold 29-language source tree indexing with a snapshot download and atomic database swap — no re-parse of the whole tree.
- **Trunk Publication**: Automated post-merge webhook or CI step that builds and publishes canonical snapshots for master/main.
- **Deduplicated Storage**: Content-addressed snapshot storage with configurable per-repo retention limits.
- **Zero-Cloud Dependency**: Designed for private clouds, local VPCs, or self-hosted bare metal servers.
- **Transport Authentication**: `weave-registry --auth-token <token>` requires a matching `Authorization: Bearer` header on every request; unset by default (loopback-trust only). Client-side: `[hub] token` in `.weave/config.toml`.
- **Snapshot Verification (`hub-provenance`)**: `weave-registry --provenance-key <secret>` rejects a push whose `X-Weave-Signature` doesn't verify against that shared secret — never the bundled verifier's public default key. Unset by default (pushes unverified, as before this existed).


## `slm`

The local-model feature is intentionally not described as a production
capability yet. Model selection, downloads, grounding, resource limits, and
quality evaluation remain open; see [unverified claims](../unverified_claims.md).

## `rbac`

Enterprise Role-Based Access Control enforcing code confidentiality and organizational boundaries:

- **Query-Layer Masking**: Enforces visibility directly inside the graph traversal and storage boundary. CLI queries (`weave query`), reports (`weave report`), exports (`weave export`), and MCP tools (`weave serve --mcp`) all inherit the identical security guard.
- **Role Scoping**: Only `"internal"` is special-cased — that identity sees everything. Every other role name (`engineer`, `admin`, `contractor`, or anything else) gets identical masked behavior: public API symbols visible, internal implementation redacted. There's no per-role permission grant beyond that one bit.
- **SCIM 2.0 Directory Server**: `weave rbac serve-scim` runs a loopback SCIM endpoint for generic subject/role provisioning and writes to `.weave/rbac-directory.toml`; optionally requires an `Authorization: Bearer <token>` (`[rbac.scim] token`) on every request. Vendor-specific SSO integration is not included.
- **Identity Invocation**: Global `--as <identity>` flag enables testing and auditing views for specific users or roles. `weave serve --mcp --require-as` (or `[rbac] require_identity = true`) refuses to start a session at all without one — for shared/multi-tenant deployments where an unmasked session by omission is unacceptable.
- **Waiver Gating**: `"allow-drift"` is the other special-cased role — it grants permission to waive a `check-contracts`/`blast` CI gate. Once granted to anyone, an identity-less waiver attempt is rejected outright.

## `policy-lint`

Declarative architectural boundary enforcement and drift detection:

- **Boundary Rules (`.weave/policy.yaml`)**: Define explicit `disallow` and `require` constraints between architectural layers (e.g., forbidding UI modules from importing database drivers directly).
- **CI Gate (`weave policy lint`)**: Evaluates the indexed graph against declared boundary rules, exiting non-zero on any violation to block offending pull requests.
- **Architectural Drift Analytics (`weave policy drift`)**: Uncovers structural decay over time, identifying dependency cycles (via Tarjan's SCC), orphaned files, and unreferenced internal symbols.
- **MCP tool (`weave_policy_lint`)**: exposes the same boundary evaluation to AI agents over MCP, masked through the session's bound identity — see [MCP Integration](mcp-integration.md).

## `otel`

Distributed trace span ingestion and graph latency overlay:

- **OTLP Trace Import**: `weave traces import <trace.json>` parses OpenTelemetry OTLP/JSON export files from Jaeger, Datadog, or OpenTelemetry Collector without requiring a live network collector.
- **Performance Graph Overlay**: Correlates runtime trace spans with static AST graph symbols (`code.function` or span names), tracking call frequencies, error rates, and latency percentiles (p50, p95, p99).
- **Latency Traversal Queries**: Query runtime performance directly through `weave query "latency(AuthService.verify)"` to detect performance regressions and bottleneck hot spots.

## `fts`

Fast, offline lexical code search using SQLite FTS5:

- **BM25 Ranking**: High-speed keyword matching over symbol names, doc comments, signatures, and file paths.
- **AST Synonym Expansion**: Automatically expands camelCase, snake_case, and language-specific conventions to maximize recall.
- **Zero-Network Execution**: Instant symbol lookups without embedding models or cloud dependencies via `weave search "<query>"`.

## `vector` (storage groundwork; learned semantic quality unverified)

Semantic code retrieval over AST-bounded chunks:

- **Syntactic Chunking**: Breaks code strictly along AST definitions (functions, classes, traits) rather than arbitrary byte boundaries.
- **Vector Storage**: Integrated vector similarity search using `sqlite-vec` virtual tables.
- **MCP tool (`weave_search_semantic`)**: exposes the same search to AI agents over MCP; a masked top hit is filtered out before the result is truncated to `limit`, never after, so it can't starve a visible runner-up out of a size-capped response — see [MCP Integration](mcp-integration.md).

## `turso` (library-only, not a CLI capability)

An alternate `Storage` backend on embedded libSQL, implementing the exact
same trait as the default `rusqlite` backend (same schema, same
migrations, same transaction discipline) — real, tested code
(`crates/weave-graph-store-turso`), but not wired into anything yet.

- **Not yet reachable from any `weave` command**: `weave-graph-cli` always
  opens `SqliteStorage` regardless of which storage features are compiled
  in; `--features turso` *adds* the backend as an optional dependency, it
  does not replace or exclude `weave-graph-store-sqlite` (a plain,
  non-optional dependency of `weave-graph-cli` either way). There is no
  `[storage.turso]` config table and no CLI flag to select a backend.
- **Integration status**: a future CLI selector requires a deliberate build
  and distribution design, plus compatibility, performance, and recovery
  tests. It is not a supported runtime configuration today.

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
