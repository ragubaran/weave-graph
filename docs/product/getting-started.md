# Getting Started

## Install

`weave` is available via package managers or can be built directly from source:

For a version-pinned Homebrew or release-archive smoke test, use [Test-install 1.0.1](install-1.0.1-testing.md).

### Package Managers

```bash
# Homebrew (macOS & Linux)
brew tap weave-graph/tap && brew install weave

# Cargo (crates.io / Rust Toolchain)
# Recommended team build (includes Single mode, Multi-repo federation, and Markdown docs)
cargo install weave-graph-cli --features team

# Enterprise build (includes RBAC, OTel, Policy Linting, Hub Registry, and Vector search)
cargo install weave-graph-cli --features custom

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
cp target/release/weave ~/.local/bin/
```

- `--features team` (`docs` + `federation` + `fts` + `vector`) is an optional developer/team
  profile, not required for a single repository.
- The core-size target applies to `cargo build --release -p
weave-graph-cli --no-default-features`; the current macOS build is
  10,128,832 bytes (9.66 MiB). Cargo-default extended-language artifacts are
  larger and must be measured separately.
- `--features custom` builds the full self-hosted enterprise suite. See [Self-Hosted](self-hosted.md) and [Features](features.md).

---

## 1. Single Repository Mode — Local Developer Workflow

No optional features required. Operates 100% locally with zero cloud egress.

```bash
cd my-repo
weave init --mode single       # writes .weave/config.toml, auto-registers .mcp.json, updates .gitignore/.ignore
weave index                    # indexes configured languages into .weave/graph.db
weave query "callers(AuthService.verify)"
weave report                   # writes WEAVE_REPORT.md + interactive .canvas file
weave serve --mcp              # starts local MCP server for AI coding agents
```

`.weave/` is where `weave` keeps its index — `weave init` configures `.weave/*` and `!.weave/config.toml` in `.gitignore` and/or `.ignore` so that `.weave/config.toml` can be tracked in version control while derived cache databases are ignored. In addition, `weave init` automatically provisions `.mcp.json` with the `weave` MCP server configuration, enabling seamless zero-config setup for AI coding assistants (Claude Code, Cursor, Windsurf, Antigravity, Gemini, etc.) while non-destructively preserving existing servers like `graft`.

### Querying the Graph

`weave query` provides deterministic traversal expressions:

```bash
weave query "callers(AuthService.verify)"     # Inbound calls (who calls this symbol)
weave query "callees(AuthService.verify)"     # Outbound calls (what this symbol calls)
weave query "impact(AuthService.verify)"      # Transitive blast radius across all hops
weave query "path(main, AuthService.verify)"  # Shortest call chain between two symbols
```

### Keeping the Index Current

```bash
weave index --incremental     # Reindexes only changed files
weave status                  # Summary: files, symbols, edges, and pending markers
```

Every `weave index` run takes an advisory file lock (`.weave/index.lock`). If two processes run concurrently (e.g. pre-commit hook and an IDE agent), the second process safely **blocks and waits its turn**. Reads (`weave query`, `weave serve --mcp`) run concurrently without blocking via SQLite WAL mode.

---

## 2. Multiple Repository Mode — Federation Across Local Repos

Requires `--features team` (or `federation`). Composes separate repository graphs **locally without a centralized server or network traffic**.

### Initializing Multi-Repo Workspaces

In each repository of your ecosystem:

```bash
cd services/auth-service
weave init --mode multiple
weave index

cd ../../services/payment-service
weave init --mode multiple
weave index
```

`--mode multiple` configures the `[federation]` section in `.weave/config.toml`:

```toml
# services/payment-service/.weave/config.toml
mode = "multiple"

[federation]
linked_repos = ["../auth-service"]
staleness_policy = "strict"   # warn | strict | ignore
```

### Linking Repositories

Link repositories to compose their graphs and establish cryptographic contract baselines:

```bash
weave link services/payment-service services/auth-service
```

- Discovers cross-repo call edges and API dependencies.
- Runs Tarjan's Strongly Connected Components (SCC) to detect multi-repo circular dependencies.
- Computes SHA-256 contract hashes of each repository's exported public API and records them as mutual expectations.

### Cross-Repository Queries

Query across the combined multi-repo boundary:

```bash
weave query-federated services/payment-service services/auth-service "callers(AuthService.verify)"
```

Traces call paths starting in `auth-service` that are triggered by handlers in `payment-service`.

### Unified Multi-Repo Architecture Canvas

Render an Obsidian JSON Canvas covering all linked repositories:

```bash
weave report-federated services/payment-service services/auth-service --out FEDERATED_MAP.canvas
```

### Automated CI Contract Gates

Enforce contract compatibility in CI pull requests before merging breaking changes:

```bash
cd services/payment-service
weave check-contracts --diff --scoped
```

- `--diff`: Displays symbol-level added, changed, and removed API declarations.
- `--scoped`: Validates only the exact subset of symbols imported by the consuming repository, avoiding false alarms on unrelated changes.
- Exits non-zero on violations when `staleness_policy = "strict"`.

### Cross-Repo Deprecation Migration Planning

Before deprecating an API in a shared library or service, generate an impact and migration plan:

```bash
cd services/auth-service
weave plan-migration --symbol "TokenValidator.verify_v1"
```

Outputs every file, line number, and consuming symbol across all linked repositories that must be updated.

---

## 3. Self-Hosted & Enterprise Custom Mode

For centralized team infrastructure, private cloud VPCs, or compliance environments, Weave Graph provides the **Custom Mode Profile** (`--features custom`).

- **Role-Based Access Control (RBAC)**: Query-layer security masking across CLI, reports, and MCP tools. Identity comes from `.weave/config.toml`'s `[rbac.users]`, loopback SCIM provisioning, or (with the optional `github-auth` feature) a GitHub token in `WEAVE_GITHUB_TOKEN`.
- **Architectural Policy Linting**: Declare architectural layers in `.weave/policy.yaml` and gate CI pull requests with `weave policy lint`.
- **Distributed Traces & Telemetry**: Import OTLP JSON traces (`weave traces import`) to overlay p50/p95/p99 latencies directly on graph symbols.
- **Schema Registry & Snapshot Hub**: Centralized snapshot synchronization with the standalone `weave-registry` daemon — atomic file-swap hydration (`weave sync pull`) instead of a cold re-parse.

👉 **Read the comprehensive [Self-Hosted & Enterprise Deployment Guide](self-hosted.md) for full configuration, setup instructions, and architecture patterns.**

---

## Next Steps

- **[CLI Reference](cli-reference.md)** — Complete command reference grouped by Profile/Tier.
- **[Configuration Reference](configuration.md)** — `.weave/config.toml` options and environment variables.
- **[Features](features.md)** — In-depth breakdown of optional Cargo features.
- **[Self-Hosted Guide](self-hosted.md)** — Deployment, query-layer RBAC, generic SCIM provisioning, and policy linting.
- **[MCP Integration](mcp-integration.md)** — Setting up Claude Code, Cursor, or Windsurf with `weave serve --mcp`.
