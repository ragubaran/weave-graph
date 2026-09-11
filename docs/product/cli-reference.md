# CLI Reference

Every command below is a real `clap` subcommand and always shows up in
`weave --help`, even when its feature isn't compiled in — running one that
needs a feature you didn't build with prints a clear message and exits
non-zero, never a silent no-op and never clap's generic "unrecognized
subcommand":

```
Error: `<command>` requires the `<feature>` feature, which is not compiled into this binary.
Rebuild with `--features <feature>`, or install the prebuilt `weave`/`weave-custom` variant.
```

Commands with no feature listed are always available.

## `weave init`

```
weave init [--mode single|multiple]
```

Writes `.weave/config.toml` and gitignores `.weave/`. `--mode multiple`
additionally scaffolds a `[federation]` section and (with a `team`/`custom`
build) an L1 GitHub Actions cache snippet at `.weave/ci-cache.yml`.

## `weave index`

```
weave index [--path <dir>] [--incremental] [--watch]
```

Builds or rebuilds `.weave/graph.db` from the repository's source tree.
`--incremental` reuses the existing index and touches only files changed
since the last run, falling back to a full rebuild once the change set is
large enough that a rebuild is cheaper (see [Configuration](configuration.md#index)).
`--watch` (feature: `watch`) runs in the foreground, debouncing file-change
events into incremental reindexes and deferring any change whose blast
radius crosses a configured ceiling behind a visible marker instead of
reindexing regardless — see [Features → watch](features.md#watch).

## `weave status`

```
weave status [--path <dir>]
```

Prints indexed file/symbol/edge counts, plus any pending markers: a
deferred large-blast-radius change (see `watch`) or files still inside the
debounce window.

## `weave serve`

```
weave serve --mcp [--transport stdio|http] [--host <addr>] [--port <port>] [--allow-remote]
```

Starts the MCP server AI agents connect to. `--transport stdio` (default)
is what most MCP clients expect; `--transport http` binds `127.0.0.1` by
default (Core Invariant: MCP never binds beyond loopback without an
explicit `--allow-remote`, since the graph exposes full source structure).
See [MCP Integration](mcp-integration.md) for client setup and the full
tool list.

## `weave query`

```
weave query "<expression>" [--path <dir>]
```

Four expression forms: `callers(symbol)`, `callees(symbol)`,
`impact(symbol)` (full transitive blast radius), `path(a, b)` (shortest
call chain). See [Getting Started](getting-started.md#querying-the-graph).

## `weave report`

```
weave report [--path <dir>] [--html] [--open]     # --html/--open need feature: viz
```

Writes `WEAVE_REPORT.md` and a JSON Canvas export (opens natively in
Obsidian's Graph View). `--html` (feature `viz`) additionally renders a
standalone, offline HTML viewer bundle; `--open` launches it in the system
browser. See [Features → viz](features.md#viz).

## `weave viz` (feature: `viz`)

```
weave viz [--open <bool>] [--port <port>] [--path <dir>]
```

Re-opens or re-serves the viewer bundles from an existing report (run
`weave report` first). `[viz] mode = "server"` in config switches from a
static `file://` bundle to a loopback-only static file server.

## `weave export`

```
weave export --symbol <name> [--depth <n>] [--path <dir>]
```

Exports a symbol's N-hop neighborhood (default depth 2) as JSON.

## `weave blast`

```
weave blast --base <ref> [--format md|json] [--out <file>] [--path <dir>]
```

PR blast-radius comment mode: diffs the current branch against `<ref>`
(three-dot / merge-base diff, so it never blames a PR for commits `<ref>`
picked up after the branch point), then unions every touched symbol's
impact radius into one markdown or JSON report. Prints to stdout by
default, or `--out <file>` — `weave` itself never talks to GitHub; pipe the
output into `gh pr comment` from CI. Refuses a shallow (`fetch-depth: 1`)
checkout with a message naming the fix, rather than a confusing raw `git`
error.

## `weave config`

```
weave config set <key> <value> [--path <dir>]
weave config get <key> [--path <dir>]
```

Reads or writes a scalar (string/bool/number) dotted key in
`.weave/config.toml`, e.g. `weave config set storage.home /path/to/vault`.
Array-valued keys like `[federation] linked_repos` need a direct file edit
— see [Configuration](configuration.md).

## `weave link` (feature: `federation`)

```
weave link [repo-a] [repo-b]
```

Composes two already-indexed repos' subgraphs locally (no network),
detects cross-repo dependency cycles, and records each repo's contract
hash as the other's expectation. With zero or one path given, the missing
repo is resolved from `[federation] linked_repos` in config.

## `weave check-contracts` (feature: `federation`)

```
weave check-contracts [--path <dir>]
```

Recomputes each linked repo's exported-API contract hash and compares it
against the expectation `weave link` recorded. Behavior is set by
`[federation] staleness_policy`: `warn` (diagnostic only), `strict`
(non-zero exit — the CI gate), `ignore`.

## `weave sync` (feature: `hub`)

```
weave sync pull [--commit <sha>] [--fallback-latest] [--path <dir>]
weave sync push [--path <dir>]
```

Client for an optional, self-hosted snapshot hub: `pull` hydrates the
graph for a commit (defaulting to `git merge-base origin/main HEAD`),
`push` publishes the current snapshot (refuses off the default branch).
There is no bundled server — this is the client side only.

## `weave ask` (feature: `slm`)

```
weave ask "<question>" [--dry-run] [--json] [--path <dir>]
```

Natural-language query for a human at a terminal — routes the question to
a `weave query`-equivalent call using a local, deterministic-by-default
router (falls back to a small local model if one is pulled via
`weave slm pull`), always prints the resolved call before running it, and
never dispatches a parameter that doesn't resolve to a real symbol. Never
used on the MCP/agent path — agents already emit exact tool calls.

## `weave slm` (feature: `slm`)

```
weave slm pull <model> --sha256 <digest>   # download + checksum-verify a model
weave slm list                             # registry models + download status
weave slm doctor                           # routing self-check against a held-out prompt set
weave slm review-rules [--confirm <idx>] [--reject <idx>] [--path <dir>]
```

## `weave journal` (feature: `slm`)

```
weave journal [--since <ref>] [--path <dir>]
```

Synthesizes a `git diff` plus the graph delta it produced into a
changelog-style summary.

## `weave note` (feature: `notes`)

```
weave note pin [--keep] [--kind <category>] <symbol> <text> [--path <dir>]
weave note list [--path <dir>]
```

Pins a short note onto a graph symbol so a later session (human or agent)
sees it without re-deriving it. Ephemeral by default (24h TTL); `--keep`
crystallizes it (never expires, gets content-hash staleness tracking
instead). Notes survive their symbol being purged and reinserted on
reindex (reattached by moniker); a note whose symbol was actually deleted
is reported with an `orphaned` tag (alongside its tier and any staleness
tag, e.g. `#3 [crystallized, orphaned] PaymentQueue: ...`), never silently
dropped.
