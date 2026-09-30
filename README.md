# weave-graph

Ultra-lightweight, memory-safe code intelligence and knowledge-federation engine, built in pure Rust. Deterministic core — no LLM, no network, no cloud — with an embedded SQL store, CSR graph model, Tree-sitter AST extraction, and a local MCP server for AI agents.

Target envelope for the core-only build: <15MB binary and <80MB RAM at 500k+ symbols. The core artifact target has a prior local measurement below 15MB; complete whole-pipeline RSS and release reproducibility remain verification work (see [Release Notes](docs/product/release-notes.md#known-limitations)).

All workspace tests pass, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` clean, line coverage ≥90% across the workspace.

**Documentation**: this file covers architecture and building from source. For day-to-day usage — install, quickstart, the full CLI/configuration reference, per-feature guides, and MCP integration — see **[`docs/product/`](docs/product/README.md)**.

## Architecture

- **`weave-graph-core`** — domain types (`Node`, `Edge`), the `Storage` trait (backend-agnostic persistence), and `CsrGraph` (integer-compacted CSR adjacency, `roaring` bitmap traversal). No I/O, no network — every other crate depends on this one, never the reverse.
- **`weave-graph-parse`** — tree-sitter grammars behind per-language extractors producing `WiringCard` symbols and raw call/structural references. A `ProjectIndex` resolves those references to edges by short symbol name. Monikers are a deterministic per-repo key (`path#qualified.symbol`).
- **`weave-graph-store-sqlite`** — the default `Storage` implementation (`rusqlite`, bundled SQLite, WAL mode). Schema versioning via a migration runner that refuses to open a database newer than the binary understands.
- **`weave-graph-store-turso`** — an alternate `Storage` implementation on embedded libSQL, same trait, same schema/migrations (feature: `turso`). Library-only — not available in any CLI build. External Rust code can use it directly via the crate.
- **`weave-graph-mcp`** — Model Context Protocol server exposing 7 unconditional deterministic tools (`weave_repo_map`, `weave_file_api`, `weave_trace_calls`, `weave_impact_radius`, `weave_check_freshness`, `weave_explore`, `weave_verify`), plus `weave_pin_note`/`weave_recall_notes` under `notes`, `weave_search_semantic` under `vector`, `weave_policy_lint` under `policy-lint`, and `weave_find_all` under `fts` — over stdio and loopback HTTP, with live reload on external reindex and optional per-tool token budgeting.
- **`weave-graph-cli`** — main `weave` executable binary: CLI commands, query engine, and every Phase 2 feature module (`docs`, `federation`, `notes`, `watch`, `viz`, `blast`, `slm`, `hub`'s client).
- **`weave-graph-hub`** — a from-scratch, dependency-free HTTP/1.1 client for an optional, self-hosted snapshot service (`weave sync pull|push`, feature: `hub`). Client only; no bundled server.
- **`weave-graph-python`** — PyO3 bindings packaged as a separate `pip install`-able wheel (feature: `python`); zero dependency from the native `weave` binary.

The SQL store is authoritative; the CSR graph is a derived read structure rebuilt from it on every load — there is no path to sync a CSR mutation back to SQL. Everything beyond the deterministic core is a Cargo feature, off by default.

## Features

Everything beyond the deterministic core is an off-by-default Cargo feature. Enabling one never changes the meaning of core behavior, and compiling a feature you don't use costs nothing — no code linked in, no idle RSS, no latency change on the default paths.

**Correction (this pass):** the table below previously had a "Mode" column mixing two unrelated things — `weave init --mode` (a runtime setting) and build tier (a compile-time one) — under values like "Any"/"Multiple"/"Custom", which doesn't cleanly map to either axis (see [`docs/mode_matrix.md`](docs/mode_matrix.md) for the full untangling). Replaced with **Tier** (which prebuilt binary, if any, ships this) and **Default or Optional** (is it already compiled into that tier's prebuilt binary, or does it need an explicit `--features` rebuild even within that tier).

| Feature | Tier | Default or Optional | Required Flag | Enables | Status |
| :--- | :--- | :--- | :--- | :--- | :--- |
| _(none)_ | Both (core) | **Default** in every build | Not required | Local AST indexing for compiled languages, CSR graph, incremental reindexing with dangling-edge purge, storage safety, query engine, MCP server, and CLI configuration | **Done** |
| `storage` (config, not a Cargo feature) | Both (core) | **Default** in every build | Not required | Custom data directory (`[storage] home` / `WEAVE_HOME`), central multi-repo knowledge store with isolated updates, network filesystem detection | **Done** |
| `docs` | Standard (`weave`) + Custom (`weave-custom`) | **Default** in both prebuilt binaries (part of `team`) | `--features docs` only if building without `team`/`custom` | Markdown/Obsidian ingestion (`pulldown-cmark`), wikilinks, `EXPLAINS_RATIONALE` code-reference links, `.canvas` export | **Done** |
| `federation` | Standard + Custom | **Default** in both (part of `team`) | `--features federation` only if building without `team`/`custom` | Multi-repo subgraph composition, composite keys, boundary contract hashing, Tarjan's-SCC cycle handling, `weave check-contracts` — **local-only, no network** | **Done** |
| `fts` | Standard + Custom | **Default** in both (part of `team`) | `--features fts` only if building without `team`/`custom` | BM25 lexical search, `weave_find_all` | **Done** |
| `vector` | Standard + Custom | **Default** in both (part of `team`; implies `fts`) | `--features vector` only if building without `team`/`custom` | Semantic/ANN search | **Done** |
| `hub` | Custom (`weave-custom`) only | **Default** in `weave-custom`; **Optional** (needs a from-source rebuild) on Standard | `--features hub` | Snapshot hydration + delta publish over the network (`weave sync pull/push`) — **client only**, no bundled hub server | **Done** |
| `hub-provenance` | Custom only | **Default** in `weave-custom`; **Optional** on Standard | `--features hub-provenance` (implies `hub`) | HMAC/Ed25519 snapshot signing | **Done** |
| `provenance` | Custom only | **Default** in `weave-custom`; **Optional** on Standard | `--features provenance` | `ProvenanceProvider` trait and note/link provenance primitives | **Done** |
| `rbac` | Custom only | **Default** in `weave-custom`; **Optional** on Standard | `--features rbac` | `AuthProvider` trait + **query-layer** node masking (never export-only) | **Done** |
| `otel` | Custom only | **Default** in `weave-custom`; **Optional** on Standard | `--features otel` | OpenTelemetry/APM trace overlay on graph nodes | **Done** |
| `policy-lint` | Custom only | **Default** in `weave-custom`; **Optional** on Standard | `--features policy-lint` | YAML architectural boundary rules + CI gate | **Done** |
| `notes` | Neither — standalone | **Optional** on both; not in any prebuilt binary | `--features notes` (with `team` or `custom`) | Pinned agent/human notes on graph symbols (`weave note pin/list`, MCP `weave_pin_note`/`weave_recall_notes`), content-hash staleness tracking, moniker-based reattachment on reindex | **Done** |
| `slm` | Neither — standalone | **Optional** on both; not in any prebuilt binary | `--features slm` | Local NL→query intent router for a human at a terminal (`weave ask`, `weave journal`) — never on the MCP/agent path. See [`docs/slm_status.md`](docs/slm_status.md) | **Done** |
| `watch` | Neither — standalone | **Optional** on both; not in any prebuilt binary | `--features watch` | Debounced auto-reindex on file change (`weave index --watch`, background thread in `weave serve --mcp`) with a blast-radius safety gate | **Done** |
| `viz` | Neither — standalone | **Optional** on both; not in any prebuilt binary | `--features viz` | Offline HTML report viewer (`weave report --html`, `weave viz`), zero new dependencies | **Done** |
| `http-compression` | Neither — standalone | **Optional** on both; not in any prebuilt binary | `--features http-compression` | MCP HTTP transport gzip negotiation | **Done** |
| `github-auth` | Structurally Custom-only (hard Cargo dependency on `rbac`) | **Optional** even on `weave-custom` — not in its prebuilt binary either | `--features github-auth` (pulls in `rbac` unconditionally via Cargo feature unification) | GitHub-token identity source (`WEAVE_GITHUB_TOKEN`) for RBAC | **Done** |
| `pr-review` | Custom (`weave-custom`) only | **Default** in `weave-custom`; **Optional** on Standard | `--features pr-review` | Consolidated, risk-scored PR review artifact (`weave pr-review`): deterministic blast-radius risk header plus contract/policy/phantom-symbol findings under a `blocker`/`warning`/`info` severity model, `--waive`/`--fail-on`. See [`docs/proposal-pr.md`](docs/proposal-pr.md) — approved and fully built | **Done** |
| `python` | Separate artifact, not `weave`/`weave-custom` | N/A — its own build | `--features python` | PyO3 bindings for a `pip install`-able wheel (not currently published — see the Installation section above) — a genuinely separate build artifact, zero cost to the native binary | **Done** |
| `turso` | Not a `weave-graph-cli` feature at all | N/A | Library crate only, no `--features` flag exists for it | Tested embedded libSQL `Storage` implementation; not available in any CLI build | **Library complete; CLI open** |

`weave blast --base <ref>` (PR blast-radius comments) needs no feature flag — it's part of the base CLI.

Convenience bundles (wired in `weave-graph-cli/Cargo.toml`): `team = [docs, federation, fts, vector]` (the `weave` binary), `custom = [team, hub, hub-provenance, provenance, rbac, otel, policy-lint, fts, vector]` (the `weave-custom` binary). See [`docs/product/features.md`](docs/product/features.md) for a full usage guide to every feature above, and [`docs/mode_matrix.md`](docs/mode_matrix.md) for the complete tier/feature cross-reference including which shared features behave differently across tiers.

## Usage

### Installation

**Correction (this pass):** this section previously listed Homebrew, crates.io, npm, and PyPI as install channels. None of them exist — verified against `.github/workflows/release.yml`, the only publish pipeline in this repo: it builds cross-platform binaries and uploads them to a **GitHub Release** only. There is no `cargo publish` step, no npm `package.json` anywhere in this repo, no PyPI/`maturin publish`/`twine` step, and no Homebrew tap. `docs/product/getting-started.md` repeats the same four fabricated channels — out of scope to fix here since it's under `docs/product/`, flagged separately.

What actually works today:

#### 1. Download a GitHub Release binary

Prebuilt binaries for Linux (glibc/musl, x86_64/arm64), macOS (Intel/Apple Silicon), and Windows are attached to each [GitHub Release](https://github.com/ragubaran/weave-graph/releases) — both the `weave` (`team` features) and `weave-custom` (`custom` features) variants, per `release.yml`'s build matrix. Download, verify against the release's `SHA256SUMS.txt`, and put the binary on your `$PATH`.

#### 2. Cargo, directly from this Git repository

```bash
cargo install --git https://github.com/ragubaran/weave-graph.git weave-graph-cli --features team
```

This works today without any crates.io publish — `cargo install --git` builds straight from source. (`cargo install weave-graph-cli` with no `--git` would require a crates.io publish that has not happened; don't use that form.)

#### 3. Build from Source

Two release distribution tiers cover every operational mode:

| Tier                 | Binary            | Build Command                                                | Key Capabilities & Target                                                                                                             |
| :------------------- | :---------------- | :----------------------------------------------------------- | :------------------------------------------------------------------------------------------------------------------------------------ |
| **Standard Tier**    | `weave` (default) | `cargo build --release -p weave-graph-cli --features team`   | Small orgs, startups & devs: Single + Multiple mode (AST parsing, SQLite, contract diffing, blast radius, BM25 search, vector search) |
| **Self-Hosted Tier** | `weave-custom`    | `cargo build --release -p weave-graph-cli --features custom` | Self-hosted & enterprise teams: Full suite (team + hub + rbac + policy-lint + provenance + otel + fts + vector)                       |

```bash
git clone https://github.com/ragubaran/weave-graph.git
cd weave-graph
cargo build --release -p weave-graph-cli --features team
```

The binary lands at `target/release/weave`; put it on your `$PATH` (e.g. `cp target/release/weave ~/.local/bin/`) or run it in place as `./target/release/weave`. The core-size target applies to an explicit `cargo build --release -p weave-graph-cli --no-default-features` build; Cargo-default extended-language builds are larger.

---

### Detailed Comparison: Tiers vs. Modes

Weave Graph separates **packaging & binary size** (Compile-Time Tiers) from **repository topology** (Runtime Modes in `.weave/config.toml`):

**Correction (this pass):** this table previously listed a fabricated third mode, "Mode 3: Custom (`mode = "custom"`)". `weave init --mode` has exactly two valid values, `single` and `multiple` — `InitMode` (`crates/weave-graph-cli/src/main.rs`) is a `clap::ValueEnum` with only those two variants; `--mode custom` is a hard parse error, not a third mode. `weave-custom` is a **build tier** (the `custom` Cargo feature bundle — `rbac`, `policy-lint`, `hub`, `hub-provenance`, `provenance`, `otel`, plus everything `team` already has), completely orthogonal to which of the two real init modes a repo uses; the two axes were conflated into one fictional "mode" here. `hub`/RBAC/`policy-lint`/`otel` capabilities are already covered correctly in the Self-Hosted Tier row below, for both real modes — this correction only removes the invented third column, not the real feature descriptions.

| Dimension                             | Mode 1: Single (`mode = "single"`)                                                                                                           | Mode 2: Multiple (`mode = "multiple"`)                                                                                              |
| :------------------------------------ | :------------------------------------------------------------------------------------------------------------------------------------------- | :---------------------------------------------------------------------------------------------------------------------------------- |
| **Primary Scope**                     | Monorepo or standalone service.                                                                                                              | Distributed local repositories via `[federation] linked_repos`.                                                                     |
| **Standard Tier (`weave`)**           | **Full Support (Default)**<br/>• Local AST index & contract hashing<br/>• Fast blast radius & reachability<br/>• SQLite backend              | **Full Support (with `federation`)**<br/>• Cross-repo moniker resolution<br/>• Tarjan SCC cycle checks<br/>• Public contract hashes |
| **Self-Hosted Tier (`weave-custom`)** | **Supported, additionally**<br/>• RBAC query masking (`--as`)<br/>• Local boundary `policy-lint`<br/>• OpenTelemetry runtime overlays<br/>• Optional self-hosted `hub` sync/registry, provenance primitives (all deployment-verification required, not turnkey) | **Supported, additionally**<br/>• RBAC masking across local repos<br/>• Local + linked contract drift gates<br/>• Same optional `hub`/provenance components as Single mode |

#### Internal Subdivisions in Standard Tier (`weave`)

1. **Storage Backend**: SQLite (bundled `rusqlite` WAL mode) is the only backend selected by the `weave` CLI. `weave-graph-store-turso` is library-only and has no CLI selector.
2. **Search Engine**: BM25 symbol search (`fts`) plus an optional vector similarity path (`--features vector`). ANN and learned semantic quality are not claimed.

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

**Zero-Config AI Agent Integration**: `weave init` automatically creates or merges into `.mcp.json` at your repository root (`"weave": { "command": "weave", "args": ["serve", "--mcp"] }`), ready for immediate use by Claude Code, Cursor, Windsurf, Google Antigravity, Gemini Code Assist, GitHub Copilot, Codex, OpenCode, Hermes Agent, and Kiro while preserving any existing servers (like `graft`). It also configures `.gitignore` and `.ignore` with `.weave/*` and `!.weave/config.toml` so `.weave/config.toml` is tracked and committable in version control while derived database and rebuild files are cleanly ignored.

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

### The `weave-custom` build tier — not a third mode

**Correction (this pass):** this was previously headed "Custom mode," implying a third `weave init --mode` value alongside Single and Multiple. There is no such mode — `--mode` only ever accepts `single`/`multiple` (see the correction in the tiers-vs-modes table above). `weave-custom` is a **build tier** (`--features custom`), layered orthogonally on top of *either* real init mode, not a mode of its own: query-layer RBAC masking, generic SCIM provisioning, architectural-boundary policy linting, an optional self-hosted snapshot hub, optional shared-secret provenance checks, and OpenTelemetry trace overlays. Verify each component's deployment requirements before use.

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
model = "qwen2.5-coder-0.5b"        # never auto-upgraded; weights not bundled; default if unset
```

**Correction (this pass):** this example previously showed `model = "qwen2.5-coder-0.5b-q4_k_m"` (wrong — the real registry name, and `ask.rs`'s own `DEFAULT_MODEL`, is `"qwen2.5-coder-0.5b"` with no quantization suffix) and a `lazy_load = true` key that **does not exist anywhere in the code** — `docs/product/configuration.md` already correctly states this ("Lazy loading... is unconditional code behavior, not a config toggle — there is no `lazy_load` key"); README just hadn't caught up to it. See `docs/slm_status.md` for the full `slm` feature review.

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
