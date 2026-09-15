# weave-graph Documentation

This directory is the published, user-facing documentation for `weave` — the
only part of `docs/` that ships in the repository (everything else is
internal planning/audit material, gitignored on purpose).

- **[Getting Started](getting-started.md)** — install, initialize a repo, multi-repo federation, run queries.
- **[Test-install 1.0.1](install-1.0.1-testing.md)** — verify Homebrew or release-archive installation before trying it on a repository.
- **[CLI Reference](cli-reference.md)** — every command and flag, divided by profile and tier.
- **[Configuration Reference](configuration.md)** — `.weave/config.toml`, section by section across all tiers.
- **[Features](features.md)** — optional Cargo features, profiles, and capabilities.
- **[Self-Hosted & Custom Mode](self-hosted.md)** — enterprise deployment, RBAC, SCIM 2.0 identity provisioning, policy linting, telemetry, and snapshot registry.
- **[MCP Integration](mcp-integration.md)** — wiring `weave serve --mcp` into Claude, Cursor, or any MCP-speaking agent.
- **[CI/CD Pipelines](pipeline.md)** — GitHub Actions integration, L1 cache setup, single mode, multiple mode, and enterprise gates.
- **[Release Notes](release-notes.md)** — release history and quality gates.


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

## Installation

`weave` can be installed via your preferred package manager or built from source:

### Homebrew (macOS & Linux)
```bash
brew tap weave-graph/tap
brew install weave
```

### Cargo (crates.io / Rust Toolchain)
```bash
# Recommended default build (Single + Multiple mode federation)
cargo install weave-graph-cli --features team
```

### npm / npx (Zero-Install for AI Agents & Node.js)
```bash
# Instant execution without pre-installation (e.g. for MCP server configuration)
npx @weave-graph/cli serve --mcp

# Global installation
npm install -g @weave-graph/cli
```

### Python / pip (`pip`)
```bash
pip install weave-graph
```

### Build from Source
```bash
git clone https://github.com/ragubaran/weave-graph.git
cd weave-graph
cargo build --release -p weave-graph-cli --features team
cp target/release/weave ~/.local/bin/
```

See the repository root [`README.md`](../../README.md) for the architecture
overview and build instructions; this directory is where the day-to-day
*usage* documentation lives.
