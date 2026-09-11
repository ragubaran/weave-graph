# MCP Integration

`weave serve --mcp` is a Model Context Protocol server exposing the
indexed graph to AI coding agents — no LLM call, no network by default,
purely deterministic tool responses computed from `.weave/graph.db`.

## Starting the server

```bash
weave serve --mcp                        # stdio transport (what most MCP clients expect)
weave serve --mcp --transport http        # loopback HTTP (127.0.0.1:8080 by default)
```

The HTTP transport binds `127.0.0.1` only, by design — binding beyond
loopback requires an explicit `--allow-remote` flag, since the graph
exposes full source structure. There is no supported reason to pass
`--allow-remote` on a shared or untrusted network.

## Connecting a client

Any MCP client that speaks stdio JSON-RPC can launch `weave` directly.
Example (Claude Desktop / Claude Code style config):

```json
{
  "mcpServers": {
    "weave": {
      "command": "/path/to/weave",
      "args": ["serve", "--mcp"],
      "cwd": "/path/to/your/repo"
    }
  }
}
```

`weave serve` has no `--path` flag — it always operates on the current
working directory, so `cwd` in the client config above is the only way to
point it at a repo. That repo must already be initialized and indexed —
run `weave init` and `weave index` first.

## Tools

| Tool | Purpose | Notable parameters |
| :--- | :--- | :--- |
| `weave_repo_map` | Progressive architectural orientation (~200 tokens) | `max_files`, `module` (Louvain module-level view instead of per-file), `max_tokens` |
| `weave_file_api` | Micro wiring cards for requested files (~60 tokens/file) | `paths` (required), `max_tokens` |
| `weave_trace_calls` | Incoming/outgoing call chains up to N hops | `symbol` (required), `depth`, `max_tokens` |
| `weave_impact_radius` | Full transitive blast radius for a proposed change | `symbol` (required), `max_tokens` |
| `weave_pin_note` *(feature: `notes`)* | Pin a note onto a symbol | `symbol`, `text` (required), `tier`, `kind` |
| `weave_recall_notes` *(feature: `notes`)* | Recall live pinned notes (expired ephemerals filtered, orphans reported) | — |

`weave_pin_note`/`weave_recall_notes` only appear in `tools/list` when the
binary was compiled with `--features notes` — the tool count an agent sees
is 4 without it, 6 with it.

## Token budgeting

Every tool above `weave_pin_note`/`weave_recall_notes` accepts an optional
`max_tokens: integer`. When the full-detail response would exceed it, the
tool sheds to a coarser tier instead of returning an unbounded response:

- `weave_repo_map`: switches from a `max_files` file-count cap to a
  token-estimate cap that sheds lines to fit.
- `weave_file_api`: full wiring cards → symbol names only → counts.
- `weave_trace_calls`: full chains → truncated chains with an explicit
  `"and N more"` marker.
- `weave_impact_radius`: full per-symbol list → file-level summary →
  module-level summary.

Omitting `max_tokens` returns exactly the same output as before this
capability existed — no existing integration needs to change.

## Staying current: live reload and staleness markers

A long-lived MCP session (an editor left open for hours) doesn't go stale
even when something else changes the index underneath it:

- If a *different* process (a manual `weave index`, or the `watch`
  background thread) rewrites the active database, the server detects it
  (a cheap, rate-limited `stat()` check) and transparently closes and
  reopens its connection before answering the next request — never a
  query against a now-stale handle.
- If the `watch` feature is enabled and a large change was deferred
  behind its blast-radius ceiling, every tool response carries an extra
  line: `⚠️ N symbols' worth of blast radius pending — run weave index to
  refresh (files: ...)`.
- If files changed moments ago but are still inside the debounce window
  (not yet reindexed), tool responses carry: `ℹ️ N file(s) just changed,
  not yet reindexed (still inside the debounce window): ...`.

Both markers are additive — they append to the tool's normal response
content, they never replace or block it.

## Determinism

Every tool response is computed directly from the on-disk graph — no
model call, no non-deterministic ranking, no network. The `slm` feature's
natural-language router (`weave ask`) is explicitly a separate, human-facing
CLI path and is never invoked on the MCP/agent side.
