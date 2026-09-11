# Getting Started

## Install

`weave` is available via package managers or can be built directly from source:

### Package Managers
```bash
# Homebrew (macOS & Linux)
brew tap weave-graph/tap && brew install weave

# Cargo (crates.io / Rust Toolchain)
cargo install weave-graph-cli --features team

# npm / npx (Zero-install instant execution for AI agents)
npx @weave-graph/cli serve --mcp

# Python / pip (CLI + in-memory bindings)
pip install weave-graph
```

### Build from Source
Requires a Rust 2024-edition toolchain (`rustc 1.93+`):

```bash
git clone https://github.com/ragubaran/weave-graph.git
cd weave-graph
cargo build --release -p weave-graph-cli --features team
```

The binary lands at `target/release/weave`. Put it on your `$PATH`:

```bash
cp target/release/weave ~/.local/bin/
```

`--features team` (`docs` + `federation`) is the recommended default build —
it covers Single mode and Multiple mode. A bare `cargo build --release`
with no `--features` also works and produces a smaller binary that only
supports Single mode. See [Features](features.md) for every other flag and
[CLI Reference](cli-reference.md) for the full command list.

## Single mode — one repo, one developer

No Cargo features required.

```bash
cd my-repo
weave init --mode single       # writes .weave/config.toml
weave index                    # builds .weave/graph.db from your source tree
weave query "callers(AuthService.verify)"
weave report                   # writes WEAVE_REPORT.md + a .canvas file
weave serve --mcp              # local MCP server for AI agents (stdio, loopback-only)
```

`.weave/` is where `weave` keeps its state — it's yours to `.gitignore`
(`weave init` adds the entry automatically) since it's a derived index, not
source of truth.

### Querying the graph

`weave query` understands four expression forms:

```bash
weave query "callers(AuthService.verify)"   # who calls this symbol
weave query "callees(AuthService.verify)"   # what this symbol calls
weave query "impact(AuthService.verify)"    # full transitive blast radius
weave query "path(main, AuthService.verify)" # shortest call chain between two symbols
```

### Keeping the index current

```bash
weave index --incremental   # touch only files changed since the last index
weave status                # summary: file/symbol/edge counts, pending markers
```

An incremental reindex falls back to a full rebuild automatically once the
changed-file ratio crosses a configurable threshold — see
[Configuration](configuration.md#index). For automatic reindexing on file
save, see the `watch` feature in [Features](features.md#watch).

Every `weave index` run takes an advisory file lock
(`.weave/index.lock`) for the duration of the write, so two concurrent
`weave index` invocations against the same repo (e.g. a pre-commit hook
and a CI job racing each other) never interleave writes. A second
invocation **blocks and waits its turn** rather than failing — it prints
`waiting for indexer (PID <pid>)...` and proceeds once the first finishes.
Reads (`weave query`, `weave serve --mcp`, etc.) are never blocked by
this lock; SQLite's WAL mode already allows any number of concurrent
readers alongside the one writer.

## Multiple mode: several local repos, no hosted service

Needs a build with `federation` (the default `team` bundle already
includes it — see Install above). Composes each repo's already-indexed
graph locally; no server, no network call.

```bash
(cd repo-a && weave init --mode multiple && weave index)
(cd repo-b && weave init --mode multiple && weave index)

weave link repo-a repo-b     # records each repo's contract hash as the other's expectation
```

`weave link` records contract expectations but doesn't yet write
`linked_repos` back into `config.toml` for you — add it once, by hand
(`--mode multiple` already scaffolds the `[federation]` section):

```toml
# repo-a/.weave/config.toml
[federation]
linked_repos = ["../repo-b"]
staleness_policy = "strict"   # warn | strict | ignore
```

```bash
cd repo-a && weave check-contracts   # CI gate on divergent public API boundaries
```

## Next steps

- [Features](features.md) — every optional capability (Markdown ingestion,
  pinned agent notes, the file watcher, PR blast-radius comments, an
  offline HTML viewer, snapshot sync, natural-language querying, alternate
  storage backends, Python bindings).
- [MCP Integration](mcp-integration.md) — connect `weave serve --mcp` to
  Claude Code, Claude Desktop, Cursor, or any other MCP client.
- [CLI Reference](cli-reference.md) — every command and flag, in one place.
