# weave-graph

Ultra-lightweight, memory-safe code intelligence and knowledge-federation engine, built in pure Rust. Deterministic core — no LLM, no network, no cloud — with an embedded SQL store, CSR graph model, Tree-sitter AST extraction, and a local MCP server for AI agents.

Target envelope for the default build: <15MB binary, <80MB RAM at 500k+ symbols. RAM is **met** (~40.7MB measured at 500k symbols, real margin under the 80MB ceiling — the Core Invariant); binary size is **not yet met** (~43MB stripped release, root-caused to 29 languages' tree-sitter grammar tables — a documented target rather than a Core Invariant; see [Release Notes](docs/product/release-notes.md#known-limitations)).

All workspace tests pass, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` clean, line coverage ≥90% across the workspace.

**Documentation**: this file covers architecture and building from source. For day-to-day usage — install, quickstart, the full CLI/configuration reference, per-feature guides, and MCP integration — see **[`docs/product/`](docs/product/README.md)**.

## Architecture

- **`weave-graph-core`** — domain types (`Node`, `Edge`), the `Storage` trait (backend-agnostic persistence), and `CsrGraph` (integer-compacted CSR adjacency, `roaring` bitmap traversal). No I/O, no network — every other crate depends on this one, never the reverse.
- **`weave-graph-parse`** — tree-sitter grammars behind per-language extractors producing `WiringCard` symbols and raw call/structural references. A `ProjectIndex` resolves those references to edges by short symbol name. Monikers are a deterministic per-repo key (`path#qualified.symbol`).
- **`weave-graph-store-sqlite`** — the default `Storage` implementation (`rusqlite`, bundled SQLite, WAL mode). Schema versioning via a migration runner that refuses to open a database newer than the binary understands.
- **`weave-graph-store-turso`** — an alternate `Storage` implementation on embedded libSQL, same trait, same schema/migrations (feature: `turso`). Both backends can link into one binary at once; not yet wired into the CLI's own storage selection.
- **`weave-graph-mcp`** — Model Context Protocol server exposing deterministic tools (`weave_repo_map`, `weave_file_api`, `weave_trace_calls`, `weave_impact_radius`, plus `weave_pin_note`/`weave_recall_notes` under `notes`) over stdio and loopback HTTP, with live reload on external reindex and optional per-tool token budgeting.
- **`weave-graph-cli`** — main `weave` executable binary: CLI commands, query engine, and every Phase 2 feature module (`docs`, `federation`, `notes`, `watch`, `viz`, `blast`, `slm`, `hub`'s client).
- **`weave-graph-hub`** — a from-scratch, dependency-free HTTP/1.1 client for an optional, self-hosted snapshot service (`weave sync pull|push`, feature: `hub`). Client only; no bundled server.
- **`weave-graph-python`** — PyO3 bindings packaged as a separate `pip install`-able wheel (feature: `python`); zero dependency from the native `weave` binary.

The SQL store is authoritative; the CSR graph is a derived read structure rebuilt from it on every load — there is no path to sync a CSR mutation back to SQL. Everything beyond the deterministic core (`docs`, `federation`, `hub`, `provenance`, `notes`, `watch`, `viz`, `slm`, `python`, `turso`, and the not-yet-started `rbac`/`otel`/`policy-lint`) is a Cargo feature, off by default.

## Features

Everything beyond the deterministic core is an off-by-default Cargo feature. Enabling one never changes the meaning of core behavior, and compiling a feature you don't use costs nothing — no code linked in, no idle RSS, no latency change on the default paths.

| Feature       | Mode              | Required Flag            | Enables                                                                                                                                                                                         | Status      |
| :------------ | :---------------- | :----------------------- | :---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :---------- |
| _(none)_      | Any               | Not Required             | Local AST & config indexing (29 languages), CSR graph, incremental reindexing with dangling-edge purge, storage safety, query engine, MCP server (stdio & loopback HTTP, live reload, token-budgeted responses), and CLI configuration | **Done**    |
| `storage`     | Any               | Not Required             | Custom data directory (`[storage] home` / `WEAVE_HOME`), central multi-repo knowledge store with isolated updates, network filesystem detection                                                 | **Done**    |
| `docs`        | Any               | `--features docs`        | Markdown/Obsidian ingestion (`pulldown-cmark`), wikilinks, `EXPLAINS_RATIONALE` code-reference links, `.canvas` export                                                                          | **Done**    |
| `federation`  | Any               | `--features federation`  | Multi-repo subgraph composition, composite keys, boundary contract hashing, Tarjan's-SCC cycle handling, `weave check-contracts` — **local-only, no network**                                   | **Done**    |
| `provenance`  | Multiple          | `--features provenance`  | `ProvenanceProvider` trait + Merkle-signed note linking                                                                                                                                         | **Done**    |
| `notes`       | Any               | `--features notes`       | Pinned agent/human notes on graph symbols (`weave note pin/list`, MCP `weave_pin_note`/`weave_recall_notes`), content-hash staleness tracking, moniker-based reattachment on reindex             | **Done**    |
| `watch`       | Any               | `--features watch`       | Debounced auto-reindex on file change (`weave index --watch`, background thread in `weave serve --mcp`) with a blast-radius safety gate                                                         | **Done**    |
| `viz`         | Any               | `--features viz`         | Offline HTML report viewer (`weave report --html`, `weave viz`), zero new dependencies                                                                                                          | **Done**    |
| `hub`         | Multiple / Custom | `--features hub`         | Snapshot hydration + delta publish over the network (`weave sync pull/push`) — **client only**, no bundled hub server                                                                          | **Done**    |
| `slm`         | Any               | `--features slm`         | Local NL→query intent router for a human at a terminal (`weave ask`, `weave journal`) — never on the MCP/agent path                                                                             | **Done**    |
| `python`      | Any               | `--features python`      | PyO3 bindings for the `pip install weave-graph` wheel — a genuinely separate build artifact, zero cost to the native binary                                                                    | **Done**    |
| `turso`       | Any               | `--features turso`       | Alternate `Storage` backend on embedded libSQL — same schema/trait as the default; not yet wired into CLI storage selection                                                                    | **Done**    |
| `rbac`        | Custom            | `--features rbac`        | `AuthProvider` trait + **query-layer** node masking (never export-only)                                                                                                                         | Not started |
| `otel`        | Custom            | `--features otel`        | OpenTelemetry/APM trace overlay on graph nodes                                                                                                                                                  | Not started |
| `policy-lint` | Custom            | `--features policy-lint` | YAML architectural boundary rules + CI gate                                                                                                                                                     | Not started |

`weave blast --base <ref>` (PR blast-radius comments) needs no feature flag — it's part of the base CLI.

Convenience bundles (wired in `weave-graph-cli/Cargo.toml`): `team = [docs, federation]`, `custom = [team, hub, provenance]` — `rbac`/`otel`/`policy-lint` join `custom` once they're implemented; none of the three exist as code or a feature flag yet. See [`docs/product/features.md`](docs/product/features.md) for a full usage guide to every feature above.

## Usage

### Installation

`weave` can be installed across multiple package channels depending on your environment:

#### 1. Homebrew (macOS & Linux)
```bash
brew tap weave-graph/tap
brew install weave
```

#### 2. Cargo (crates.io / Rust Toolchain)
```bash
# Recommended default variant (Single + Multiple mode federation)
cargo install weave-graph-cli --features team

# Or directly from the Git repository
cargo install --git https://github.com/ragubaran/weave-graph.git weave-graph-cli --features team
```

#### 3. npm / npx (Zero-Install for AI Agents & Node.js)
```bash
# Run immediately via npx without pre-installing (e.g. for MCP server configuration)
npx @weave-graph/cli serve --mcp

# Or install globally
npm install -g @weave-graph/cli
```

#### 4. Python / pip (`pip`)
```bash
# Installs CLI executable on virtualenv PATH + exposes in-memory query bindings
pip install weave-graph
```

#### 5. Build from Source
Two release distribution tiers cover every operational mode:

| Tier | Binary | Build Command | Key Capabilities & Target |
| :--- | :--- | :--- | :--- |
| **Standard Tier** | `weave` (default) | `cargo build --release -p weave-graph-cli --features team` | Small orgs, startups & devs: Single + Multiple mode (AST parsing, SQLite/Turso, contract diffing, blast radius) |
| **Self-Hosted Tier** | `weave-custom` | `cargo build --release -p weave-graph-cli --features custom` | Self-hosted & enterprise teams: Full suite (Custom mode + RBAC, SCIM, Hub, Policy-Lint, Provenance, Vector, SLM, OTel) |

```bash
git clone https://github.com/ragubaran/weave-graph.git
cd weave-graph
cargo build --release -p weave-graph-cli --features team
```

The binary lands at `target/release/weave`; put it on your `$PATH` (e.g. `cp target/release/weave ~/.local/bin/`) or run it in place as `./target/release/weave`. A bare `cargo build --release` (no `--features`) yields a minimal, Single-mode-only binary (<15MB, zero network).

---

### Detailed Comparison: Tiers vs. Modes

Weave Graph separates **packaging & binary size** (Compile-Time Tiers) from **repository topology** (Runtime Modes in `.weave/config.toml`):

| Dimension | Mode 1: Single (`mode = "single"`) | Mode 2: Multiple (`mode = "multiple"`) | Mode 3: Custom (`mode = "custom"`) |
| :--- | :--- | :--- | :--- |
| **Primary Scope** | Monorepo or standalone service. | Distributed local repositories via `[federation] linked_repos`. | Centralized organization-wide mesh & private VPC infrastructure. |
| **Standard Tier (`weave`)** | **Full Support (Default)**<br/>• Local AST index & contract hashing<br/>• Fast blast radius & reachability<br/>• Choice of SQLite or Turso backend | **Full Support (with `federation`)**<br/>• Cross-repo moniker resolution<br/>• Tarjan SCC cycle checks<br/>• Public contract hashes | **Not Supported**<br/>(Requires enterprise Hub registry, SCIM server, and RBAC guard). |
| **Self-Hosted Tier (`weave-custom`)** | **Supported + Enterprise Add-ons**<br/>• RBAC query masking (`--as`)<br/>• Local boundary `policy-lint`<br/>• OpenTelemetry runtime overlays | **Supported + Enterprise Add-ons**<br/>• RBAC masking across local repos<br/>• Local + linked contract drift gates<br/>• Local SLM journal generation | **Full Native Support**<br/>• Centralized Hub registry & spool queues<br/>• SCIM 2.0 Okta/Azure AD sync<br/>• LOD 1 Mermaid architecture diagrams<br/>• Merkle snapshot provenance verification |

#### Internal Subdivisions in Standard Tier (`weave`)
1. **Storage Backend**: `sqlite` (bundled `rusqlite` WAL mode, default) vs `turso` (`--features turso`, embedded libSQL remote replica).
2. **Search Engine**: Non-vector BM25 symbol search (`fts`, default) vs Semantic Vector Search (`--features vector`, 1-bit `sqlite-vec` ANN + int8 rescore).

---

### Single mode — one repo, one developer

The default. No Cargo features required.

```bash
cd my-repo
weave init --mode single     # writes .weave/config.toml, auto-configures .mcp.json, updates .gitignore/.ignore
weave index                  # builds .weave/graph.db
weave query "callers(AuthService.verify)"
weave report                 # WEAVE_REPORT.md + .canvas export
weave serve --mcp            # local MCP server for AI agents (stdio, loopback-only)
```

**Zero-Config AI Agent Integration**: `weave init` automatically creates or merges into `.mcp.json` at your repository root (`"weave": { "command": "weave", "args": ["serve", "--mcp"] }`), ready for immediate use by Claude Code, Cursor, Windsurf, Google Antigravity, Gemini Code Assist, GitHub Copilot, Codex, OpenCode, and Hermes Agent while preserving any existing servers (like `graft`). It also configures `.gitignore` and `.ignore` with `.weave/*` and `!.weave/config.toml` so `.weave/config.toml` is tracked and committable in version control while derived database and rebuild files are cleanly ignored.

### Multiple mode — several local repos, no hosted service

Needs the default `weave` binary (`--features team`, which includes `federation`) — see Install above. Composes each repo's already-indexed graph locally — no server, no network.

```bash
# index both repos first — weave link reads each one's own graph.db
(cd repo-a && weave init --mode multiple && weave index)
(cd repo-b && weave init --mode multiple && weave index)

weave link repo-a repo-b     # records each repo's contract hash as the other's expectation
```

`weave link` records contract expectations but doesn't yet write `linked_repos` back into `config.toml` for you, so before `check-contracts` can run, edit `repo-a/.weave/config.toml` (`--mode multiple` already scaffolds the `[federation]` section):

```toml
[federation]
linked_repos = ["../repo-b"]
staleness_policy = "strict"   # warn (diagnostic only) | strict (non-zero exit, the CI gate) | ignore
```

```bash
cd repo-a && weave check-contracts   # CI gate on divergent public-API boundaries
```

### Custom mode — enterprise add-ons

Layers enterprise capabilities on top of Multiple mode for self-hosted VPCs and regulated orgs: query-layer RBAC masking (`--features rbac`), SCIM 2.0 directory sync (`weave rbac serve-scim`), an architectural-boundary policy linter (`--features policy-lint`), centralized snapshot hub (`--features hub`), Merkle provenance verification (`--features hub-provenance`), and OpenTelemetry trace overlays (`--features otel`).

## Configuration

Everything lives in `<repo>/.weave/config.toml`, written by `weave init`. Every section besides `mode` is optional — an absent section means that capability is off, never a pending setup step. This is a summary; see [`docs/product/configuration.md`](docs/product/configuration.md) for the full reference, including `[watch]`/`[viz]`/`[report]`:

```toml
mode = "single"                     # single | multiple

[storage]
home = ""                           # optional central/custom storage path (default: <repo>/.weave/)
relocate_on_network_fs = false      # opt-in; default is warn-and-refuse

[index]
bailout_ratio = 0.10                # full rebuild above this share of changed files
bailout_floor = 100                 # ...but never below this absolute count

[federation]                        # requires feature = federation
linked_repos = []                   # e.g. ["../sibling-repo"]
staleness_policy = "warn"           # warn | strict | ignore

[hub]                               # requires feature = hub; expected to be rare
url = ""                            # unset is a fully supported permanent state

[slm]                               # requires feature = slm
model = "qwen2.5-coder-0.5b-q4_k_m" # never auto-upgraded; weights not bundled
lazy_load = true                    # must stay true: 0MB idle cost until first `weave ask`
```

Read or write a scalar key via the CLI:

```bash
weave config set storage.home /path/to/central/knowledge/repo-a
weave config get storage.home
```

`weave config set` only writes scalar (string) values — array keys like `[federation] linked_repos` need a direct edit to `.weave/config.toml`.

### Central knowledge store (`[storage] home` & `WEAVE_HOME`)

To store graph knowledge outside the repository tree (e.g. a central folder or multi-repo knowledge vault), either set `[storage] home` per repo (above) or export a global default:

```bash
export WEAVE_HOME=/path/to/central/knowledge
```

When `WEAVE_HOME` is set and no repo-specific `[storage] home` is set, `weave` automatically isolates each repository's database under a sanitized namespace: `$WEAVE_HOME/<sanitized-repo-path>/`. Indexing only ever touches the target repository's own database and swap files (`graph.db`, `.rebuild`, advisory lock) — other repositories under the same central folder are unaffected. With neither `[storage] home` nor `WEAVE_HOME` set, storage defaults to `<repo_root>/.weave/`.

## Building

### Whole Workspace

```bash
# Debug build (all workspace crates)
cargo build --workspace

# Release build (optimized, stripped binary)
cargo build --workspace --release
```

### Per-Crate / Module Build

```bash
# Core graph models & CSR data structures
cargo build -p weave-graph-core

# Tree-sitter & AST parsers / extractors
cargo build -p weave-graph-parse

# SQLite storage layer
cargo build -p weave-graph-store-sqlite

# CLI binary
cargo build -p weave-graph-cli

# MCP server
cargo build -p weave-graph-mcp

# Optional crates (when features are enabled)
cargo build -p weave-graph-hub
cargo build -p weave-graph-store-turso
```

## Testing

```bash
# Run all workspace tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p weave-graph-parse
cargo test -p weave-graph-core
cargo test -p weave-graph-store-sqlite

# Run a specific integration test file
cargo test -p weave-graph-parse --test fixture_extraction
cargo test -p weave-graph-parse --test r_extraction
cargo test -p weave-graph-parse --test web_extraction
```

## Code Coverage

We require **≥90% line coverage** across all crates verified by `cargo-llvm-cov`.

### Prerequisites

```bash
# Install cargo-llvm-cov and LLVM tools preview
cargo install cargo-llvm-cov
rustup component add llvm-tools-preview
```

### Whole Workspace Coverage

```bash
# Check full workspace coverage with 90% threshold gate
cargo llvm-cov --workspace --all-targets --fail-under-lines 90

# Generate interactive HTML coverage report
cargo llvm-cov --workspace --html --open

# Generate lcov report (for CI)
cargo llvm-cov --workspace --lcov --output-path lcov.info
```

### Per-Crate / Module Coverage

```bash
# weave-graph-core
cargo llvm-cov -p weave-graph-core --fail-under-lines 90

# weave-graph-parse
cargo llvm-cov -p weave-graph-parse --fail-under-lines 90

# weave-graph-store-sqlite
cargo llvm-cov -p weave-graph-store-sqlite --fail-under-lines 90

# weave-graph-cli
cargo llvm-cov -p weave-graph-cli --fail-under-lines 90

# weave-graph-mcp
cargo llvm-cov -p weave-graph-mcp --fail-under-lines 90

# weave-graph-hub
cargo llvm-cov -p weave-graph-hub --fail-under-lines 90

# weave-graph-store-turso
cargo llvm-cov -p weave-graph-store-turso --fail-under-lines 90
```

### Pre-Commit Local Verification Check

```bash
cargo fmt --check && \
cargo clippy --workspace --all-targets -- -D warnings && \
cargo test --workspace && \
cargo llvm-cov --workspace --fail-under-lines 90
```

## Rules

See `AGENTS.md` for the non-negotiable invariants (crash-resilient indexing, feature isolation, RBAC enforcement point, coverage minimums) every change in this repo must hold to.

## License

MIT — see `LICENSE`.
