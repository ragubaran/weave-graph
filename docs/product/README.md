# weave-graph Documentation

This directory is the published, user-facing documentation for `weave` — the
only part of `docs/` that ships in the repository (everything else is
internal planning/audit material, gitignored on purpose).

- **[Getting Started](getting-started.md)** — install, initialize a repo, run your first query.
- **[CLI Reference](cli-reference.md)** — every command and flag.
- **[Configuration Reference](configuration.md)** — `.weave/config.toml`, section by section.
- **[Features](features.md)** — what each optional Cargo feature adds, and how to use it.
- **[MCP Integration](mcp-integration.md)** — wiring `weave serve --mcp` into Claude, Cursor, or any MCP-speaking agent.
- **[Release Notes](release-notes.md)** — what's in the first release, and what's known-incomplete.

## What is weave-graph?

`weave` builds a queryable code graph for a repository — symbols, call
edges, and structural relationships — using deterministic static analysis
(Tree-sitter parsing, no LLM, no network) and serves it to both a human
(CLI queries, Markdown/Canvas reports) and AI coding agents (a local Model
Context Protocol server). The deterministic core has no optional
dependency: every capability beyond it — Markdown ingestion, multi-repo
federation, pinned agent notes, a file watcher, natural-language querying,
alternate storage backends, Python bindings — is an off-by-default Cargo
feature that costs nothing when not compiled in.

See the repository root [`README.md`](../../README.md) for the architecture
overview and build instructions; this directory is where the day-to-day
*usage* documentation lives.
