# AGENTS.md: Development Rules & Engineering Standards

> **Scope**: Mandatory for all AI coding agents (Antigravity, Claude Code, Cursor) and human contributors working in this repository.  
> **Project**: `weave-graph` (CLI: `weave` / Crates: `weave-graph-*`, see §6)  
> **Core Mission**: Build an ultra-lightweight (<15MB binary, <80MB RAM), memory-safe code intelligence engine in pure Rust with deterministic execution, zero cloud leaks, and >=90% test coverage.

---

## 1. Non-Negotiable Core Invariants

1. **Zero-LLM Core**: The core graph builder, parser, SQLite store, and MCP tools must execute **100% deterministically** without requiring an LLM or network connection.
2. **Crash-Resilient Indexing**: Never write dirty data into the active `.weave/graph.db`. Large rebuilds must write to `.weave/graph.db.rebuild` and atomically `rename(2)` over the active database.
3. **Bidirectional Edge Integrity**: Every incremental file reindex must purge both outbound (`source_id`) AND inbound (`target_id`) edges to prevent dangling pointers.
4. **Resource Envelope**: Peak memory must never exceed 80MB RAM for 500k symbols. Adjacency lists must use integer-compacted CSR matrices (`uint32`).
5. **No Network in Base Tier**: The default build must compile with zero network dependencies (no `reqwest`, no Tokio runtime in `weave-graph-core`).
6. **MCP Server Binds Localhost Only**: `weave serve --mcp` must default to loopback. Binding beyond localhost requires an explicit flag — the graph exposes full source structure.
7. **RBAC Is Enforced at the Query Layer, Never Export-Only**: When `rbac` is enabled, masking lives inside the storage/traversal boundary so CLI, `weave report`, exports, and MCP all inherit one guard. Adding masking only in an export path — even "temporarily" — is the specific mistake this project's design review already caught and rejected once; do not reintroduce it.
8. **Feature Isolation**: Enabling any optional feature (`docs`, `federation`, `hub`, `provenance`, `slm`, `rbac`, `otel`, `policy-lint`, `python`, `turso`) must not measurably change default-build query latency or idle RSS. This is asserted in CI, not just claimed.
9. **Ground Truth & Zero Invention**: Never invent fictional features, phantom APIs, fabricated flags, synthetic metrics, or placeholder benchmarks when creating or modifying code, tests, documentation, or marketing copy. All documentation, tests, examples, and claims must strictly reflect real, implemented ground truth verified against the active codebase.

---

## 2. Code Coverage & Testing Rules

### 2.1 Minimum 90% Line Coverage Target
* Every crate must maintain **>=90% code line coverage** verified by `cargo-llvm-cov`.
* PRs that lower overall project coverage or fall below 90% in any crate will fail CI.
* Run coverage locally before committing:
  ```bash
  cargo llvm-cov --workspace --all-targets --fail-under-lines 90
  ```

### 2.2 Testing Hierarchy
1. **Unit Tests (`src/**/tests.rs`)**:
   * Pure algorithmic verification (Tree-sitter queries, CSR bitmask intersections). Tarjan's SCC is scoped to the `federation` feature (Phase 2) — test it there, not in `weave-graph-core`'s default-build suite.
   * Mock external storage; execute in under 10ms.
   * **The one test that blocks merging any indexing change**: index two mutually-referencing files, reindex one, assert zero edge endpoints reference a missing node (Core Invariant 3).
2. **Integration Tests (per-crate `tests/`)**:
   * Storage layer transactions, schema migrations, and SQLite crash-safety.
3. **End-to-End (E2E) CLI Tests (`crates/weave-graph-cli/tests/`)**:
   * Use `assert_cmd` and `predicates` to test real binary workflows:
     ```rust
     use assert_cmd::Command;
     use predicates::prelude::*;

     #[test]
     fn test_cli_init_and_index() {
         let mut cmd = Command::cargo_bin("weave").unwrap();
         cmd.arg("init").arg("--mode").arg("single").assert().success();
     }
     ```
   * Must execute against real test fixtures in `crates/weave-graph-parse/tests/fixtures/` (Rust, TypeScript, and Python codebases).

---

## 3. Uniform Tech Stack

Strict crate boundaries ensure consistency. No ad-hoc dependencies are permitted.

| Layer | Standard Library / Crate | Purpose | Rule |
| :--- | :--- | :--- | :--- |
| **Language** | Rust 2024 Edition (`1.93+`) | Core implementation | Latest stable edition (`2024`); proactive N+1 stable tracking. |
| **AST Parsing** | `tree-sitter` (C/Rust) | Syntax extraction | Microsecond execution; no AST tree leaks across threads. |
| **Markdown** | `pulldown-cmark` | Wikilinks & ADRs | Pure CPU streaming parser; zero regex parsers. |
| **Primary Store** | `rusqlite` (`bundled`) | SQLite persistence | Default engine behind `Storage` trait. WAL mode for local `.weave/graph.db`; **non-WAL journal mode required** for any read-only shared snapshot served over a network filesystem (WAL needs shared memory, which network mounts don't provide). |
| **Graph Memory**| `petgraph::csr::Csr` + `roaring` | Compaction & bitmasks | Contiguous memory; 32-bit integer string interning. |
| **CLI Framework**| `clap` (`derive`, `v4`) | Argument parsing | Derive macros only; clean subcommand hierarchy. |
| **Concurrency** | `rayon` | Parallel AST parsing | Thread pool bounded to `num_cpus::get()`. |
| **Benchmarking**| `criterion` (`v0.5+`) | Micro-benchmarks | Standardized in each crate's `benches/`; gate regressions at 10%. |
| **Error Handling**| `thiserror` (libs), `anyhow` (CLI) | Typed errors | No `unwrap()` or `expect()` in library crates. |

---

## 4. Comment Rules (The 4-Line Maximum Rule)

Every comment in this codebase must adhere to the **Best Comment Guide**:

1. **Strict 4-Line Maximum**: No comment block or docstring may exceed **4 lines**.
2. **Explain "Why", Never "What"**: Code explains *what* is happening; comments explain *invariants, architectural decisions, and hardware constraints*.
3. **No Doc-File Citations or Dates in Code**: No comment block or docstring may cite planning/tracking documents (e.g. `impl.md`, `plan.md`), milestone codes (`M1.x`, `M2.x`, `M3.x`), gap numbers, section marks (`§`), or external spec files (`docs/*.md`) — nor calendar dates. Plain functional comments only. Citations and dates are allowed exclusively inside documentation files (`docs/` and markdown docs).
4. **Self-Documenting Code**: If a function requires more than 4 lines of explanation, refactor the function into smaller, well-named units.
5. **Example of Compliant Comment**:
   ```rust
   // Purge edges in both directions before re-inserting AST nodes.
   // Deleting only source_id leaves orphaned incoming edges from
   // other files, corrupting get_callers traversals.
   storage.purge_file_edges_bidirectional(file_id)?;
   ```

---

## 5. Google Rust Style & Engineering Conventions

All code must follow the [Google Rust Style Guide](https://google.github.io/styleguide/rustguide.html) and Google engineering practices:

### 5.1 Visibility & Scoping
* **Default to Private**: Everything is private by default.
* Use `pub(crate)` for internal inter-module boundaries. Use `pub` only for types exported in public library APIs.

### 5.2 Immutability & Variables
* Use `let` by default; use `let mut` only when mutation is strictly necessary in a small scope (<15 lines).
* Prefer iterator transformations (`map`, `filter`, `fold`) over mutable loop accumulators.

### 5.3 Function Design
* Functions must be small and single-purpose: **target <40 lines per function**.
* Arguments must use borrowed slices (`&str`, `&[T]`) rather than owned collections (`String`, `Vec<T>`) unless ownership transfer is required.

### 5.4 Safety Invariants
* Default to `#![deny(unsafe_code)]` in all crates.
* If `unsafe` is mathematically required for SIMD/CSR memory layouts:
  * Must be isolated inside a minimal function with a mandatory `// Safety: ...` invariant comment explaining why undefined behavior is impossible.

### 5.5 Error Handling: No Panics in Production
* **Library crates** (`weave-graph-core`, `weave-graph-parse`, `weave-graph-store-sqlite`, `weave-graph-store-turso`, `weave-graph-mcp`, `weave-graph-hub`, `weave-graph-python`, `weave-graph-wasm`): production code must contain no `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`, or `unimplemented!`. Every fallible call returns a typed `Result<T, thiserror::Error>` up to the caller. Test, benchmark, and example code may use test assertions and failure helpers.
* **`weave-graph-cli`** has softer rules — `anyhow` and top-level `main`/setup code may terminate with a user-facing error only after reporting the failure. `unwrap()`, `expect()`, and panic macros are prohibited in normal user-invoked paths (parsing input, handling files, and subcommand logic); use `anyhow::Context` and propagate `Result` instead.
* Panic macros are allowed only in test, benchmark, and example code; production code reports every failure through its declared error boundary.

### 5.5.1 Dead Code: Remove Before Suppressing
* Remove unreachable functions, types, fields, imports, and feature branches. Do not add `allow(dead_code)`, `expect(dead_code)`, or crate-wide dead-code suppression to hide obsolete code.
* A suppression is permitted only for a compiler-context false positive (for example, a benchmark importing a live private module by path). It must sit directly beside the suppression and state the concrete reason; remove it when that context no longer applies.

### 5.6 Imports Organization
Organize `use` statements into 3 distinct, sorted blocks separated by empty lines:
```rust
// 1. Standard library
use std::path::{Path, PathBuf};
use std::sync::Arc;

// 2. Third-party crates
use petgraph::csr::Csr;
use rusqlite::Connection;

// 3. First-party workspace crates
use weave_graph_core::model::{NodeId, Symbol};
use weave_graph_core::storage::Storage;
```

---

## 6. Project & Workspace Folder Structure

Benches, integration tests, and fixtures live **per-crate**, not at the
workspace root — standard Cargo convention, and what `cargo test`/`cargo
bench --workspace` already discover without path overrides. Each crate's
`benches/`/`tests/` covers only that crate's public API; `tests/fixtures/`
holds its pinned sample files.

```text
weave-graph/
├── Cargo.toml                     # Virtual workspace root, shared edition/rust-version
├── Cargo.lock
├── AGENTS.md                      # This development rulebook
├── .gitignore
└── crates/
    ├── weave-graph-core/          # Graph CSR data structures, AST traits, models. No I/O, no network.
    │   ├── src/
    │   └── benches/               # csr_memory.rs: RAM footprint per 1M edges
    ├── weave-graph-parse/         # Tree-sitter & Markdown parsers, wiring cards
    │   ├── src/
    │   ├── benches/               # parser_throughput.rs: Tree-sitter MB/s
    │   └── tests/                 # Integration tests + tests/fixtures/ sample corpora
    ├── weave-graph-store-sqlite/  # Default rusqlite storage implementation
    │   ├── src/
    │   └── tests/                 # Schema migration & round-trip tests
    ├── weave-graph-store-turso/   # Optional libSQL replica storage (feature: turso)
    ├── weave-graph-mcp/           # Model Context Protocol server tools
    ├── weave-graph-cli/           # Main `weave` executable binary; `slm` feature (weave ask, weave journal) lives here, NOT a separate crate
    │   └── tests/                 # e2e CLI tests (assert_cmd), once added
    └── weave-graph-hub/           # Optional delta ingestion/publish service (feature: hub). Never linked into the default binary.
```

---

## 7. Build Pipeline & CI Verification Guides

Every pull request must pass the local and CI validation pipeline:

### 7.1 Local Pre-Commit Check
Run this single one-liner before submitting any change:
```bash
cargo fmt --check && \
cargo clippy --workspace --all-targets -- -D warnings && \
cargo test --workspace && \
cargo llvm-cov --workspace --fail-under-lines 90
```

### 7.2 Release Profile Configuration (`Cargo.toml`)
Production binary builds must optimize for minimum binary size and maximum runtime throughput:
```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
overflow-checks = false
```

### 7.3 CI Workflow Matrix (`.github/workflows/ci.yml`)
1. **Lint Job**: `cargo fmt -- --check`, `cargo clippy --all-targets -- -D warnings`.
2. **Test & Coverage Job**: `cargo llvm-cov --workspace --lcov --output-path lcov.info` (Upload to codecov; fail if <90%).
3. **E2E Job**: Run test suite against real multi-language fixtures in `crates/weave-graph-parse/tests/fixtures/`.
4. **Benchmark Regression Gate**: Run `cargo bench -- --threshold 10` (Fail PR if performance regresses >10% against base commit).
5. **N+1 Forward-Compatibility Job**: Proactively test workspace against `beta` / next stable release (N+1) to catch compiler lints, deprecations, and upstream regressions before they land in stable.

---

## 8. Agent Behavior Checklist

When generating or editing code in this workspace, all agents must verify:
- [ ] Are all new comment blocks <= 4 lines explaining *why*, not *what*?
- [ ] Are code comments free of doc-file citations (`*.md`), milestone codes, and dates (plain functional comments only)?
- [ ] Is line coverage >=90% for newly created code modules?
- [ ] Are all errors handled via typed `Result<T, E>` with zero `unwrap()` calls in libraries?
- [ ] Did incremental updates purge edges bidirectionally?
- [ ] Does the storage layer use temporary file swap for large rebuilds?
- [ ] Does the binary compile cleanly with `cargo build --no-default-features`?
- [ ] Does `weave serve --mcp` still default to localhost-only bind?
- [ ] If touching `rbac`: is masking enforced at the query/storage layer, not just in an export path?
- [ ] If adding/changing a feature: does the feature-isolation check still show zero change to default-build latency and idle RSS?
- [ ] Are all documented flags, APIs, config keys, and performance claims verified against real ground-truth code (zero invented items)?

<!-- graft:start -->
## Graft — repo context graph

This repo is indexed in `graft/`: small linked markdown nodes that explain each
system and carry exact file:line spans, kept in sync with the code through git.

For ANY task here — understanding how something works, finding where code lives,
or scoping a change — get context from the graph before grepping or opening
source files. Re-ask freely (it's cheap) and reuse literal identifiers you
already have (symbol, error string, file name) as the query. New to this repo?
Run `graft map` first — a token-budgeted orientation (dir clusters, hubs,
hotspots), no LLM, no key.

- Run `graft ask "<your question>" --source` → ranked nodes with the relevant
  code spans inlined (each hit's ≤8-line crux by default; `--full` for whole
  definitions when the crux isn't enough). Match the tool to the task shape:
  for understanding or editing, the top node IS the answer — cite its
  `covers:` file:line spans and edit straight from `--source`. For
  exhaustive tasks ("every occurrence / every caller of this pattern"), ranked
  results are top-N, not complete — run `graft grep "<literal>"` instead
  (exhaustive over indexed files, grouped by enclosing symbol), falling back
  to raw `grep -rn` only for unindexed files.
- `graft skeleton <file>` → every definition's signature + span, ~10× cheaper
  than reading the file; use it to skim an API surface.
- `graft callers <symbol>` gives precomputed, exact edges — who calls this.
  Add `--direction out` for what it calls, or `--depth N` to walk
  transitively for the full blast radius. For structural questions, skip
  ranking and use this directly.
- Or browse: `graft/INDEX.md` lists every node; follow the links.
- Monorepos and folders of multiple repos rank fairly across sub-projects —
  hits carry `[scope/]` labels naming which one they're from. Narrow with
  `graft ask "<task>" --in <scope>/` once you know where you're working.

If a returned span is truncated ("+N more lines"), open the file at that exact
range before finalizing. Only open source files when a node genuinely lacks a
needed detail, and then at the exact file:line the node points to — never
re-read whole files.

After big code changes, refresh the graph with `graft build` (deterministic,
no API key, $0).
<!-- graft:end -->

@RTK.md
