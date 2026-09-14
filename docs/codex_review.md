# Consolidated Codex Review

This is the sole Codex audit record. It combines the security, indexing,
resource, model-selection, and individual-change audits formerly spread over
three `codex_review*.md` files. Findings and test counts are dated historical
evidence, not a new validation run. The live issue register is
[issues.md](issues.md); approved architecture is [plan.md](plan.md);
the independent measured comparison remains
[performance_compare.md](performance_compare.md).

**Scope:** `docs/impl.md`, `docs/plan.md`, and production Rust under `crates/`.

## Audit

This audit records the resolution of findings from the earlier security review.
Statuses describe the current working tree, including the requested fixes.

| Finding | Status | Change |
| --- | --- | --- |
| RBAC/custom builds did not compile | Fixed | The MCP handler now makes tool arguments mutable only in RBAC builds, uses the resolved guard, and builds cleanly with every feature enabled. `UserConfig` call sites now consistently access `roles`. |
| SEC-02 snapshot sanitization failed open | Fixed | Vector deletion now targets `vec_chunks.rowid` through the matching `nodes.id`, propagates every snapshot/purge error, and sync exports a read-only SQLite snapshot before sanitizing it. A regression test confirms excluded vectors are absent from the snapshot sent to the hub. |
| SEC-07 HTTP bearer identity was not connected to MCP | Fixed | HTTP `Authorization: Bearer` is checked before reading the body and is injected as transient `_meta.token` only for handler dispatch. The handler removes token metadata before tool execution, preventing reflection in arguments, logs, and responses. |
| Authentication comparisons were inconsistent | Fixed | `weave-graph-core::auth` supplies shared constant-time token comparison and bearer-header matching. MCP, Hub, and SCIM use it. Duplicate RBAC user tokens are rejected while creating the reverse subject lookup. |
| Hub/MCP could allocate or spawn without limits | Fixed | MCP and Hub cap headers and body/chunk sizes before allocation, apply socket deadlines, authenticate before buffering bodies, and the Hub limits concurrent worker threads. Oversized MCP bodies are rejected before they are read. |
| Hub admitted uploads after disk spooling | Fixed | The registry reserves capacity, rate-limit budget, target ancestry, and declared snapshot size before the first chunk is written. Reservations and temporary spools are released on every rejected/error path; incomplete live reservations expire after 15 minutes. |
| Vector index and snapshot sync retained full datasets | Fixed | Vector rebuilding streams source paths and chunks into SQLite. Moniker construction streams nodes rather than first materializing a duplicate node vector. Sync sends database snapshots from disk in bounded chunks instead of loading the database payload into memory. |
| `[hub.canvas] exclude` was dead configuration | Fixed | `weave-registry --config <path>` now reads `[hub.canvas] exclude`; explicit `--canvas-exclude` continues to take precedence. The unused CLI configuration reader was removed. |
| Plan/implementation documentation diverged from code | Fixed | The feature table now records implemented vector support, the upload protocol reflects the actual resumable `Content-Range` transport and 5 MiB chunks, and feature-isolation scope is stated precisely. |

### Changed Areas

- `weave-graph-core`: shared constant-time authentication helpers and tests.
- `weave-graph-mcp`: RBAC-safe token dispatch, pre-body bearer authentication, request limits, and HTTP transport tests.
- `weave-graph-cli`: fail-closed vector snapshot sanitization, streaming vector indexing and sync upload, and the excluded-vector snapshot regression.
- `weave-graph-store-sqlite`: streaming vector rebuild and correct vector-row purge by SQLite row ID.
- `weave-graph-hub`: bounded server resources, pre-spool admission/reservations, declared upload quotas, cleanup, streamed client upload, and configuration-backed canvas filtering.
- `docs/impl.md` and `docs/plan.md`: current vector, transport, and feature-isolation status.

### Validation Results

| Command | Result |
| --- | --- |
| `cargo fmt --check` | Passed. |
| `cargo check --workspace --all-targets --all-features` | Passed. |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Passed. |
| `cargo test -p weave-graph-core` | Passed: 38 tests across 2 suites. |
| `cargo test -p weave-graph-mcp --all-features` | Passed: 72 tests across 2 suites. |
| `cargo test -p weave-graph-store-sqlite --features vector` | Passed: 61 tests across 6 suites. |
| `cargo test -p weave-graph-hub --all-features` | Passed: 105 tests across 4 suites. |
| `cargo test -p weave-graph-cli --features custom` | Passed: 249 tests across 2 suites. |

The focused Hub and CLI suites were run with local loopback socket access because their integration tests exercise real transport behavior.

### Remaining Test-Matrix Constraint

`cargo test --workspace --all-features` currently fails at link time before tests execute: the feature combination links both bundled SQLite implementations (`libsql_ffi` and `libsqlite3_sys`) and also lacks the Python/PyO3 linker inputs in this environment. This is a workspace feature-matrix/toolchain issue, not a failure in the changed test suites. The all-feature compile and clippy checks above complete successfully.

## Indexing, Search, and Resource Follow-up

This chapter supersedes earlier performance assumptions with the current working
tree. The 15 MB requirement applies to the core-only `weave` build, without
optional features; it is not a budget for an extended feature build.

### Resolved in This Follow-up

| Area | Status | Verified change |
| --- | --- | --- |
| Core binary size | Meets target | `cargo build --release -p weave-graph-cli --no-default-features` produced a 10,145,392-byte (9.68 MiB) `weave` binary. `petgraph` no longer enables its default features in core. |
| Incremental dependency repair | Fixed | A changed file now finds affected callers through indexed SQL joins, rather than loading all graph edges or issuing per-symbol caller queries. The resolver also rechecks files that recorded previously unresolved references. |
| Incremental edge integrity | Fixed | Reindexing no longer purges an unchanged affected caller. That former behavior could delete unrelated inbound edges; a regression test preserves an upstream-to-caller edge while the caller is re-resolved. |
| Parse scheduling | Fixed | Bounded parallel parsing returns work in path order before nodes are written. This preserves deterministic ID and edge insertion order while bounding in-flight parse results. |
| FTS updates | Fixed | Node writes, node-path purges, and orphan cleanup maintain `symbol_fts` transactionally. Search fetches full nodes in ranked SQL order, avoiding a CLI-side node lookup per hit. |
| Search limits and masking | Improved | CLI and MCP semantic search cap requested results at 100. Lexical visibility filtering runs in the SQLite search boundary before the final result limit. |
| Vector rebuild memory | Improved | Source spans borrow from one file buffer during embedding instead of allocating one `String` per source line. Vector dimensions are validated as 384 before insert or search. |
| Vector incremental work | Fixed | Changed paths are purged from the vector table before old nodes are removed, then only symbols in those paths are embedded and inserted. A regression test ensures a replaced symbol's old vector cannot be returned. |

### Verification Performed

| Command | Result |
| --- | --- |
| `cargo fmt --check` | Passed after formatting the new regression tests. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed. |
| `cargo check --workspace --all-targets --all-features` | Passed. |
| `cargo test --workspace` | Passed. |
| `cargo test -p weave-graph-cli --features vector` | Passed: 158 tests. |
| `cargo test -p weave-graph-store-sqlite --features vector` | Passed. |
| `cargo test -p weave-graph-cli --features fts` | Passed. |
| `cargo build --release -p weave-graph-cli --no-default-features` | Passed; binary measured at 9.68 MiB. |

### Later corrections and remaining work

The table above is the earlier result. Later work added batched prepared writes,
span-only node-ID retention, model-ID metadata/mismatch protection, and a
core-only 15 MiB CI artifact check. It did **not** establish full-pipeline
500k-symbol RSS, production BGE inference, semantic quality, ANN, or hybrid
ranking. The corrected per-change status ledger below is authoritative for
this audit; [issues.md](issues.md) is the current gap tracker. Do not publish
unmeasured query latency, compression, RSS, recall, or ANN throughput.

## Accepted model-selection boundary

The deterministic core needs no embedding or generative model. Optional
`lite` selects `BAAI/bge-small-en-v1.5` for both semantic indexing and
semantic querying, with explicit local installation outside the core binary.
The SLM remains independently selected and is invoked only for an explicit
codebase question or feature-design request. Shared SLM/vector weights are
not supported. Provider identity is recorded and a mixed-model index is
rejected, but the actual BGE runner and complete fingerprint are not shipped.
The tested FastEmbed/ONNX Runtime dependency combination lacked an Intel
macOS prebuilt runtime; this does not rule out every possible ONNX approach.

## Why the performance follow-up was needed

The initial performance review found that a one-file incremental update
reparsed and rewrote repository-wide data, that global resolver maps and
edge de-duplication raised peak RAM, and that FTS/vector maintenance rebuilt
derived stores. Later changes addressed the logical incrementality, edge
set, FTS deltas, and vector deltas. The remaining full-database copy and
process-wide resolver are separate scalability issues; neither can be
declared solved by the improved parser throughput.

The optional vector implementation stores compressed representations and
uses a mock lexical-hash provider; a `sqlite-vec` scan is not an ANN
benchmark. The original size measurements also distinguished the core-only
artifact (about 9.7 MiB locally) from the much larger Cargo-default
extended-language artifact. Exact historical size samples and repeatable
comparisons remain in [performance_compare.md](performance_compare.md).
## Implementation Status Audit — 2026-09-14

The checkboxes in the delivery sequence are a historical implementation plan,
not proof that every part of a compound item landed. This audit records the
current tree's individual status. **Verified** means the implementation and a
relevant regression test were found; **Partial** means only the stated subset
is present; **Open** means the planned behavior was not found.

| Phase | Individual change | Status | Evidence and remaining condition |
| --- | --- | --- | --- |
| 0 | 500k-symbol resource fixture | Partial | `examples/mem_500k.rs` builds 500k real SQLite nodes and a CSR, but RSS is measured externally and no recorded/CI threshold enforces 80 MB. |
| 0 | Baseline index/search/vector/caller measurements | Open | Criterion covers smaller CSR/parser/SQLite cases, but no pinned cold/warm full-index, 1% incremental, FTS, vector, or first-caller benchmark suite was found. |
| 0 | Release-artifact size matrix | Partial | CI now fails above the 15 MiB core-only cap and uploads measured core, extended, and core-plus-vector sizes. Extended artifacts still have no approved enforced budget. |
| 0 | Planned regression coverage | Partial | Tests cover resolver repair after a new symbol, preservation of unrelated inbound edges, line-only ID retention, oversized-result limits, and FTS hidden-hit filtering. Overload-safe semantic-key migration coverage remains open. |
| 1 | Remove global edge de-duplication set | Verified | `upsert_all_edges` writes resolved edges directly and reports counts through storage; no indexing-wide edge `HashSet` remains. |
| 1 | Reuse prepared node/edge statements in batches | Verified | `SqliteStorage::upsert_nodes` and `upsert_edges` reuse cached statements for each parsed/resolved file batch; indexing calls those batch methods. |
| 1 | Persist resolver dependencies | Verified | SQLite stores indexed `unresolved_refs`; incremental reindex queries it using old and newly parsed short names. |
| 1 | Stable semantic node identity | Partial | Incremental writes now match an existing `(repo, path, symbol, kind)` candidate by exact signature before span and retain its ID across a line-only shift. Overload disambiguation without a persisted semantic key remains open. |
| 1 | Reindex only changed and affected files | Verified | The affected set combines changed paths, SQL-discovered callers, and unresolved-reference dependents; only that set is re-resolved. Regression tests protect unchanged callers' unrelated inbound edges. |
| 1 | Retain staged, atomic publication | Verified | Incremental work copies to `graph.db.rebuild` and publishes with rename semantics. The copy remains O(database size). |
| 1 | Replace full FTS rebuild on ordinary updates | Verified | Node upsert, node-path purge, and orphan purge maintain FTS transactionally; a test proves writes/purges do not require a full rebuild. The explicit rebuild method remains for repair. |
| 2 | Remove duplicate resolver string ownership and full moniker map | Open | `build_project_index_from_storage` still constructs a process-wide `ProjectIndex` and moniker-to-node map. |
| 2 | Budget/evict reverse CSR and measure it at scale | Partial | Caller traversal now builds and drops a temporary reverse CSR, so it cannot permanently raise idle RSS. The benchmark remains analytical and tops out at 200k nodes. |
| 2 | Minimize `petgraph` features | Verified | Core disables `petgraph` default features and the core-only release build remains 9.68 MiB. |
| 2 | Enforce size budgets for core and extended artifacts | Partial | CI enforces the 15 MiB core-only cap and uploads extended measurements. Separate extended-feature budgets have not been approved or enforced. |
| 3 | Incremental vector updates | Verified | Changed paths are purged before node deletion, then only those paths are embedded and written. A regression test rejects a replaced symbol's stale vector. |
| 3 | Allocation-light source slicing | Verified | Vector indexing derives spans from one source buffer and line-offset table rather than allocating strings for all source lines. |
| 3 | Vector metadata and local model selection | Partial | `vector_metadata` records the provider model ID on rebuild; incremental writes and searches refuse a mixed-model index. `lite = BAAI/bge-small-en-v1.5` is selected, and shared SLM embeddings are rejected by design. Runtime still uses `MockEmbeddingProvider`; no portable BGE runner, explicit installer, chunk hash, dimensions, or quantization-version metadata exists. FastEmbed/ONNX Runtime is unsuitable on Intel macOS because upstream lacks a prebuilt `x86_64-apple-darwin` runtime. |
| 3 | Dimension and normalization contract | Partial | The provider boundary now returns embedding errors and the SQLite path rejects dimensions other than 384. An explicit provider normalization contract and reusable embedding work buffers are not present. |
| 3 | Benchmarked on-disk ANN backend | Open | The optional `sqlite-vec` path remains the candidate mechanism; no ANN backend, recall corpus, or p95 target was found. |
| 3 | Deterministic FTS/vector hybrid ranking | Open | Lexical and semantic paths remain separate; no candidate fusion or reciprocal-rank implementation was found. |
| Search | Storage-side lexical visibility and bounded limits | Partial | FTS filtering runs before final truncation and limits cap at 100. Candidate over-fetch can still underfill results when many top candidates are masked; semantic MCP rendering still reads nodes per returned ID. |

### Model-Selection Audit Update — 2026-09-14

| Change | Status | Verified evidence |
| --- | --- | --- |
| Select `lite = BAAI/bge-small-en-v1.5` | Decision accepted | 384 dimensions preserve the existing vector table and compact payload layout. This is a product decision, not a claim that the model runner is shipped. |
| Reject shared SLM/vector weights | Decision accepted | SLM routing and semantic retrieval remain separate optional capabilities. No SLM is treated as an embedding provider. |
| Record vector provider identity | Verified | `vector_metadata` stores the identity at rebuild. Search and incremental updates reject a different identity; regression test `vector_operations_reject_a_mixed_model_index` covers both paths. |
| Keep an empty new vector index usable | Verified | Semantic search returns no results from an empty index before requiring model metadata; regression coverage protects this initialization case. |
| Ship a BGE `lite` runtime | Open | FastEmbed/ONNX Runtime was evaluated and rejected because the upstream runtime cannot build on Intel macOS (`x86_64-apple-darwin`). A portable runner and explicit local installer remain required. |

Validation performed after the metadata change:

```text
cargo fmt --check
cargo check -p weave-graph-cli --features vector
cargo test -p weave-graph-core --features vector      # 44 passed
cargo test -p weave-graph-store-sqlite --features vector # 67 passed
cargo test -p weave-graph-cli --features vector       # 158 passed
git diff --check
```

### Corrected Phase Summary

- **Phase 0:** a manual 500k fixture and automated core-size gate exist; RSS
  and performance gates are not automated.
- **Phase 1:** logical incrementality, resolver dependencies, FTS deltas,
  stable span-only IDs, batched writes, and edge-integrity tests are complete;
  persisted overload-safe semantic keys remain open.
- **Phase 2:** the core artifact target, `petgraph` reduction, artifact-size
  CI gate, and non-retaining caller traversal are complete; resolver/CSR
  scale measurement remains open.
- **Phase 3:** vector delta writes, source slicing, and model-ID mismatch
  protection are complete. BGE `lite` is the accepted profile, but its portable
  local runner, quality benchmark, chunk metadata, ANN, and hybrid retrieval
  remain open.
