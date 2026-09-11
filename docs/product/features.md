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
| [`hub`](#hub) | `--features hub` | **Done (client only)** |
| [`slm`](#slm) | `--features slm` | **Done** (deterministic scope; real-model latency pending downloaded weights) |
| [`turso`](#turso) | `--features turso` | **Done** (library backend; not yet wired into CLI storage selection) |
| [`python`](#python) | `--features python` | **Done** (separate `pip install` wheel, native binary untouched) |
| MCP live reload | not required (base MCP tier) | **Done** |
| MCP token budgeting | not required (base MCP tier) | **Done** |
| `rbac` | `--features rbac` | Not started (Phase 3) |
| `otel` | `--features otel` | Not started (Phase 3) |
| `policy-lint` | `--features policy-lint` | Not started (Phase 3) |

Convenience bundles (`weave-graph-cli/Cargo.toml`): `team = [docs, federation]`,
`custom = [team, hub, provenance]`.

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

Client for an optional, **rare** self-hosted snapshot service: `weave sync
pull` hydrates the graph for a commit (falling back to the hub's `latest`
snapshot on request), `weave sync push` publishes the current snapshot
(refuses off the default branch, retries once on a `409` conflict). This
is the client side only — `weave-graph-hub` implements a small hand-rolled
HTTP/1.1 client with zero new dependencies beyond the standard library;
there is no bundled hub server in this repository. Most teams never need
this feature — it exists for orgs wanting cross-machine warm starts
without re-indexing from scratch on every checkout.

## `slm`

A natural-language query router **for a human at a terminal**
(`weave ask`), explicitly never on the MCP/agent path (agents already emit
exact tool calls — a translation layer could only add latency and lose
fidelity). Grounds every routed parameter against the real symbol table
before dispatch — an unresolvable or ambiguous token is reported as a
routing failure, never guessed. Falls back to a deterministic heuristic
router when no local model is configured; `weave slm pull` downloads a
checksummed GGUF model for the (external, shelled-out) `llama-cli`-backed
router, `weave slm doctor` runs a held-out self-check reporting
tool-selection rate, parameter-grounding rate, and latency. Model weights
load lazily — compiling `slm` in costs 0MB idle RSS until `weave ask` is
actually invoked.

Two further `slm`-gated commands, unrelated to querying:

- `weave slm review-rules` scans every `*.md` file in the repo for
  obligation-shaped sentences ("must", "must not", "should never" —
  fenced code blocks skipped) and lists them as **candidate** ADR-style
  rules, never authoritative facts. `--confirm 1,3`/`--reject 2` (1-based
  indexes into the current listing) persist a decision to
  `.weave/rules.toml`, keyed by the candidate's exact text so re-running
  against the same Markdown is idempotent. This is standalone —
  independent of whether the `docs` feature is also compiled in.
- `weave journal [--since <ref>]` combines a `git diff` against `<ref>`
  (defaulting to `HEAD~1` when omitted) with the graph delta those
  changed files produced — which symbols were touched, their blast
  radius — into a changelog-style summary. Every figure comes from the
  already-indexed graph; no model inference in this path.

## `turso`

An alternate `Storage` backend on embedded libSQL, implementing the exact
same trait as the default `rusqlite` backend (same schema, same
migrations, same transaction discipline) — a from-scratch rewrite is not
needed to add a second engine. Both backends can be compiled into the same
binary at once (`--features turso` links `rusqlite` and libSQL together
without conflict). Batch-insert throughput is currently ~15–25% slower
than `rusqlite` on this workload (measured, not assumed), which is why it
isn't the default. Not yet wired into the CLI's own storage selection —
no `weave` command can choose it today; it exists as a library-level
alternative for embedders.

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
