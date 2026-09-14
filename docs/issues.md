# Issues and verification gaps

This is the single current issue register for the internal docs. Statuses below are
carried forward from the dated audits, not a fresh source-code verification in this
documentation-only consolidation. “Open” means the cited audit did not establish
completion; “conditional” means an explicit product or security decision is needed.
Implementation work and approved direction live in [impl.md](impl.md) and
[plan.md](plan.md). Historical rationale is retained in [codex_review.md](codex_review.md).
Do not turn proposed performance targets into measured results.

## Release-blocking and performance gaps

| ID | Status | Issue and required closure |
| --- | --- | --- |
| PERF-G01 | Open | The 500k-symbol **whole indexing process** has no repeatable peak-RSS gate proving the 80 MB envelope. The existing SQLite/CSR fixture is narrower. Add a pinned full-pipeline corpus and CI threshold. |
| PERF-G02 | Open | No pinned cold-index, unchanged-index, one-file, rename/delete, high-fanout, exact/FTS/vector, or first-caller p50/p95 suite proves throughput claims. Establish profile-specific baselines first. |
| PERF-G03 | Partial | The core-only release artifact has an automated 15 MiB check, but the repository says “<15 MB” and the MB/MiB interpretation remains unresolved. Extended, Basic, vector, and SLM distributions lack agreed independent budgets. |
| PERF-G04 | Open | Incremental publication copies the full active SQLite database to `graph.db.rebuild`, giving O(database size) I/O and temporary disk cost. Any replacement needs atomicity and recovery tests. |
| PERF-G05 | Open | The storage-derived resolver still materializes a process-wide `ProjectIndex` and moniker map. Measure and reduce it without breaking late-reference repair. |
| PERF-G06 | Partial | Span-only edits retain node IDs; overloaded declarations still lack a persisted collision-safe semantic key and migration coverage. |
| PERF-G07 | Partial | Reverse CSR is temporary, not retained at idle, but the full caller-path peak and 500k-node graph have not been measured end to end. |
| PERF-G08 | Partial | Ordered parallel parsing is bounded by channel capacity but can accumulate later results behind a slow first file. Bound retained reorder bytes and test adversarial ordering. |
| PERF-G09 | Open | FTS visibility over-fetch can underfill a requested page. Move authorization into candidate selection/refill at the storage boundary. |
| PERF-G10 | Open | Optional vector retrieval uses a mock provider; no portable BGE runner, complete embedding fingerprint, held-out code-quality result, or benchmarked ANN path exists. Do not claim production semantic relevance or ANN latency. |
| PERF-G11 | Open | Lexical and vector results have no evaluated deterministic fusion; quantization lacks float-baseline recall evidence. Treat both as optional experiments until measured. |
| PERF-G12 | Partial | Feature-isolation smoke coverage exists, but release-build, long-lived MCP idle RSS/query latency, and optional worker process-tree measurements remain open. |
| PERF-G13 | Open | Per-crate >=90% coverage and the full feature/release matrix are not established by the focused test results. The all-feature workspace test was inconclusive in the recorded environment. |
| AUTH-GH-01 | Partial | Optional `github-auth` resolves `WEAVE_GITHUB_TOKEN` through GitHub `/user`, performs bounded retry, applies `[rbac.github_roles]`, and can query `/user/orgs` for `[rbac.github_org_roles]`. Local mock coverage currently proves `/user` only; organization-response tests and team-level mapping remain open. |
| PERF-G14 | Partial | Optional MCP HTTP gzip compression is implemented, but Hub compression, unsupported-encoding fallback, small-payload crossover, and release package/RSS/CPU measurements remain unverified. |
| RELEASE-01 | Conditional | The `v1.0.0` tag had no published GitHub Release in the 2026-09-11 record: two Intel macOS build legs remained queued and the run was cancelled. Decide whether Intel artifacts block release, then re-check current remote state before acting. |

The dated RELEASE-01 record says 10 of 12 binary legs completed, while two
`x86_64-apple-darwin` legs on `macos-13` remained queued. Cancelling the run
prevented the publish job; the built artifacts were only ephemeral run
artifacts. The uncommitted product release notes were to be held until a
real release exists. The unresolved choice was to retain Intel support and
retry, make those legs non-blocking with an explicit artifact policy, or
remove Intel from the release matrix. None of these choices is approved here;
verify the current tag, workflow and remote release before editing CI.

The Phase 4 work packages and acceptance criteria for these gaps are in
[impl.md](impl.md#5-phase-4-developer-performance-evidence-based-review--optional-local-assistance).
The accepted profile/model boundaries are in [plan.md](plan.md).

## Documentation interoperability verification

| ID | Status | Issue and required closure |
| --- | --- | --- |
| DOC-OBS-01 | Open, environment-blocked | JSON Canvas export is covered by a passing schema test, but the Obsidian desktop rendering check is not complete. `/Applications/Obsidian.app` exists on the verification host, yet macOS reports `kLSNoExecutableErr` because the bundle has no Resources payload. Reinstall a complete Obsidian application, open a generated `.canvas` in a temporary vault, and record a successful render before marking this claim closed. |

## Security, storage and Phase 3 capability register

The original Phase 3 audit used some inaccurate `feature_matrix.md` claims as
its starting point. The statuses below follow its later per-issue corrections
plus the subsequent Codex security review. A feature gap is not automatically
a vulnerability. Before prioritizing a deferred row, re-check source and the
deployment threat model.

| Original ID | Status | Current interpretation / next action |
| --- | --- | --- |
| CORE-01 | Open, scope clarified | `TursoStorage` is implemented and tested as a library backend, but CLI/MCP construct SQLite and expose no backend selector. The current supported product is SQLite-only; schedule a separate Turso binary/process only if there is a concrete deployment need. |
| CORE-02 | Closed | Search methods were added to the `Storage` trait; this does **not** resolve CORE-01. |
| CORE-03 | Closed | Optional MCP semantic and policy tools were registered and feature forwarding through the CLI was fixed. |
| SEC-01 | Closed, caveat | Vector visibility is applied before final rerank truncation. Continue to test top-k behavior and timing/metadata exposure under realistic RBAC policies. |
| SEC-02 | Closed for export path | Later review fixed vector-row purge and fail-closed sanitized Hub snapshots. Shared local vector-store visibility and retention policy remain deployment decisions, not proof of an active leak. |
| SEC-03 | Closed | Vector search is exposed through the `Storage` trait and guarded call boundary. |
| SEC-04 | Closed | MCP semantic-search tool was wired with feature forwarding and masking tests. Its mock embedding is not production semantic retrieval. |
| SEC-05 | Closed for opt-in policy | `--require-as` / `[rbac] require_identity` gate MCP startup. Do not silently impose RBAC on unconfigured solo repos; verify each future transport's identity path. |
| SEC-06 | Closed for configured grants | Anonymous waivers are denied when the repo grants `allow-drift` to configured users. |
| SEC-07 | Closed | HTTP bearer identity is checked before body read, and stdio now accepts per-request `_meta.token` credentials through the same transient handler path. Added regression coverage proving a valid stdio token resolves an identity and is not reflected in output; invalid/missing credentials remain fail-closed when `require_auth` is enabled. |
| IDP-01 | Closed | RFC 7643 object-array `roles` and `groups` are ingested, with regression coverage for both flat and object-array payloads. Group values are preserved as `group:<value>` markers for policy mapping. |
| IDP-02 | Closed, opt-in | Loopback SCIM can enforce a configured bearer token; an unset token retains the earlier local unauthenticated mode. |
| RBAC-01 | Deferred capability | Path-scoped roles need an approved role-model expansion and authorization tests. |
| RBAC-02 | Closed | `[rbac.group_mappings]` maps SCIM `group:<value>` markers to Weave roles during guard construction. Direct roles are preserved, mapped roles are deduplicated, malformed mappings are ignored, and focused RBAC tests cover ingestion plus authorization. |
| POL-01 | Open | RBAC-masked policy lint can report false clean results in CI. Define whether CI uses an unmasked privileged identity or fails on incomplete visibility; do not invent an existing flag. |
| POL-02 | Deferred capability | Semantic-coupling policy checks are not implemented; require a quality and false-positive evaluation before gating CI. |
| POL-03 | Closed | Mask-induced orphan reports are annotated, using one graph fetch for both views. |
| POL-04 | Deferred capability | Role/team ownership and boundary exemptions need an approved policy schema. |
| POL-05 | Deferred capability | Waiver role hierarchy and bypass audit trail require a policy decision; do not rely on the obsolete `WEAVE_ALLOW_DRIFT` claim. |
| FED-01 | Deferred capability | Cross-repository boundary linting is not yet a local federation capability. |
| PROV-01 | Closed, opt-in | Hub registry can verify with an operator-supplied provenance key before commit; no key means no verification. Do not present the test verifier as public-key provenance. |
| HUB-01 | Partial | Hub canvas now exposes an opt-in `CanvasAuthorizer` callback that receives the request credential and filters module nodes before JSON serialization. End-to-end identity/RBAC wiring, mesh-canvas filtering, and threat-model tests remain open; static exclusion alone is not RBAC. |
| HUB-02 | Closed, opt-in | Hub bearer authentication is available, but unset-token deployments retain the unauthenticated loopback default. |
| HUB-03 | Deferred capability | Central mesh policy endpoint depends on an approved FED-01 design. |

## Closed historical resource findings

These identifiers remain here so older review notes can be traced, but their
original “resolved” labels do **not** prove the present 80 MB/latency envelope.

| Original ID | Recorded resolution or disposition |
| --- | --- |
| PERF-01 | Hub worker timeout/pruning and later concurrency/admission limits. |
| PERF-02 | WAL checkpoint and staged-swap handling. |
| PERF-03 | Registry mutex poisoning handling. |
| PERF-04 | Missing-symbol query error rather than unchecked panic. |
| PERF-05 | Iterative Tarjan SCC for the federation path. |
| PERF-06 | Chunked parse/write path; PERF-G08 and PERF-G01 remain. |
| PERF-07 | Python binding lock errors converted to Python errors. |
| SCALE-01 | Git-aware discovery was recorded; historical sub-350ms claim was not reproduced here and must not be published as current. |
| SCALE-02 | Git sentinel watcher was recorded; million-file behavior has no pinned gate here. |
| SCALE-03 | Binary/int8 vector compression exists as groundwork; this does not close PERF-G10/G11 or prove ANN. |
| SCALE-04 | Roaring-based graph work was recorded; historical microsecond claim needs a pinned benchmark. |
| SCALE-05 | CI cache restore/publish strategy was recorded; external quota assumptions require re-checking. |
| CSR-M3.9 | The earlier CSR bytes-per-node formula was corrected, unused `f64` weights were removed, and reverse CSR became lazy/temporary. The 500k **full pipeline** memory gate remains PERF-G01. Measurement details stay in [performance_compare.md](performance_compare.md), which is kept separate. |

## Documentation accuracy issues

- The plan and older reviews use “default build” inconsistently. The core size
  target applies to `--no-default-features`; Cargo defaults include extended
  languages. Every size claim must name the artifact and bytes.
- Older plans describe a hybrid/ANN/learned semantic pipeline as shipped.
  The mock provider and quantized scan are groundwork; production semantic
  quality and ANN evidence are PERF-G10/G11.
- Older issue and feature matrices contain unsupported flags, role semantics,
  platform assurances and synthetic timings. Treat examples as design
  sketches until checked against the CLI/configuration and measured artifacts.
- Product/release notes must not claim a published `v1.0.0` release until a
  release artifact exists. See RELEASE-01.

## Historical Phase 3 per-issue rationale

The following detailed findings are preserved from the 2026-09-13 audit so
the threat model and rejected remediations are not lost. Its embedded
“Fixed”/“Verified” notes describe that audit's time, and some baseline
feature-matrix claims were later corrected. The registry above, plus the
later security and performance review, takes precedence for current status.
Do not implement a proposed flag or backend enum solely from this appendix.

### 2. Core Feature & Dual-Tier Storage Gaps (CORE-01 to CORE-03)

#### CORE-01: Concrete `SqliteStorage` Coupling in CLI and MCP Server
- **⚠️ Investigated, still open (2026-09-13) — the original remediation is unsafe to build as written**: this pass set out to implement exactly the remediation below (a `StorageBackend` enum + `Deref<Target = dyn Storage>`) as part of Phase 3. Before wiring it into `open_storage_for_read`/`McpHandler`, a cross-compat check was run: open a `SqliteStorage` connection, then open a `TursoStorage` connection in the *same process*. It panics — `crates/weave-graph-store-turso/tests/cross_compat.rs::opening_turso_after_sqlite_in_the_same_process_panics` reproduces it every time, on unrelated files, even fully in-memory. Root cause: `weave-graph-store-sqlite` (`rusqlite`, feature `bundled`) and `weave-graph-store-turso` (`libsql`, feature `core`) each statically link their **own vendored `sqlite3.c`**. The two bundled libraries' symbols collide at link time (`ld: duplicate symbol '_sqlite3_prepare_v2'` and dozens more — macOS `ld` tolerates this with a warning, silently picking one), and at runtime the second library to actually open a connection hits libsql's own threading-configuration self-check and panics: `"libsql was configured with an incorrect threading configuration"`. This is not a config problem on one machine — it is data about how the two crates' native dependencies are built, confirmed by an isolated experiment before assuming it was a fluke. Because `weave-graph-cli`'s indexing/write path calls `SqliteStorage::open` unconditionally (never gated by `turso`), **any single `weave` binary that also links `weave-graph-store-turso` carries this landmine** — not a corner case, a guaranteed panic the moment any code path in that one process opens both. Shipping the `StorageBackend`/`Deref` wiring as originally described would compile clean, pass review, and then crash the first time an operator actually flips `storage.backend = turso` on. Moved to §9 Phase 4 as a decision item (needs a build-matrix choice, not just code) rather than left as ready-to-build Phase 3 work — see that section for the two real options.
- **Location**: [`crates/weave-graph-cli/src/main.rs:L1272-1291`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-cli/src/main.rs#L1272-L1291), [`crates/weave-graph-mcp/src/handler.rs:L64-85`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-mcp/src/handler.rs#L64-L85), and [`crates/weave-graph-store-turso/tests/cross_compat.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-store-turso/tests/cross_compat.rs) (the new regression test proving the conflict)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Row 28) specifies that both Standard and Self-Hosted tiers support either SQLite WAL or Turso libSQL storage. However, `open_storage_for_read` in CLI and `McpHandler` in MCP server hardcode concrete `SqliteStorage` initialization (`Result<(SqliteStorage, PathBuf), ...>` and `RefCell<SqliteStorage>`). Neither uses the `Box<dyn Storage>` trait or a `StorageBackend` dynamic dispatcher, rendering `--features turso` completely inoperative at runtime.
- **Dual-Tier Impact**:
  - `crates/weave-graph-store-turso` implements the `Storage` trait for embedded libSQL, and `weave-graph-cli/Cargo.toml` has `turso = ["dep:weave-graph-store-turso"]`.
  - However, enabling the `turso` feature in either Standard or Self-Hosted tier produces no operational effect because the CLI and MCP handlers can only construct `SqliteStorage` — and, per the finding above, cannot safely be made to construct `TursoStorage` either without a build-matrix change, since the two backends can never coexist in one linked binary.
- **Remediation**:
  - ~~Introduce a unified `enum StorageBackend { Sqlite(SqliteStorage), #[cfg(feature = "turso")] Turso(TursoStorage) }` implementing `Deref<Target = dyn Storage>`.~~ Superseded — see §9 Phase 4 item 10 for the corrected options (separate binary vs. documenting the limitation).
  - **Feasibility (2026-09-13) — corrected**: the `Deref<Target = dyn Storage>` mechanics themselves are indeed trivial (confirmed: `CORE-02`/`SEC-03`'s trait-level change this pass already makes every downstream read command `&dyn Storage`-generic, so an enum wrapper would need zero further call-site changes). What's *not* mechanical, and not a matter of effort, is that the enum's two variants can never both be exercised in one process. This was the one piece the original feasibility note didn't check before calling it "medium effort, no architectural blocker."

---

## Claims removed from user-facing product documentation

The following claims were removed or narrowed in `docs/product/*.md` during
the documentation audit. They remain listed here so their absence is not
mistaken for implementation or release approval.

| Removed claim | Reason it was removed | Tracking |
| --- | --- | --- |
| The default build is below 15 MB and the full indexing process stays below 80 MB RAM at 500k symbols. | The small target applies only to the explicit `--no-default-features` artifact; whole-pipeline RSS has no repeatable CI gate. | PERF-G01, PERF-G03, PERF-G12 |
| Vector search is production semantic search, binary ANN, or a measured hybrid BM25/vector system. | The current provider is mock groundwork; ANN, BGE quality, quantization loss, and fusion have no accepted measurements. | PERF-G10, PERF-G11 |
| BGE or another learned embedding model is bundled, portable, or quality-certified. | A real provider, installer, complete fingerprint, and Intel/macOS portability evidence are still open. | PERF-G10 |
| SLMs provide measured accuracy/TTFT, ADR extraction, autonomous refactoring, or zero-RSS guarantees. | Model artifacts and end-to-end evaluation are not part of the verified release surface. | PERF-G10; Phase 4 P4-E |
| Weave provides built-in SSO/OIDC/SAML integrations or vendor-specific IdP adapters. | Generic SCIM plus static role mapping remain available; the optional `github-auth` feature now performs GitHub token identity lookup only. Generic OAuth/OIDC, SAML, and group mapping are not shipped. | IDP-01, RBAC-02 |
| Provenance is built-in Merkle/PKI/non-repudiation protection. | Core operation is standalone and provenance is optional. The opt-in registry path uses an operator-supplied shared secret and provides integrity checking only; production Merkle/PKI requires a deployment-supplied provider. | PROV-01 |
| Turso is a selectable backend of the normal `weave` CLI. | `TursoStorage` is library-only today; no CLI selector or supported Turso distribution exists. | CORE-01 |
| Hub provides authenticated, per-identity RBAC diagrams or a centralized mesh policy endpoint. | Authentication is opt-in, canvas exclusion is not per-identity RBAC, and cross-repository policy linting is not implemented. | HUB-01, HUB-02, HUB-03, FED-01 |
| Universal sub-millisecond latency, 92% token reduction, or cross-platform certification. | These were projections or environment-specific observations without reproducible release gates for every supported profile/platform. | PERF-G02, PERF-G03, PERF-G12, PERF-G13 |

The canonical user-facing rule is: document only interfaces verified in the
current tree, and describe optional semantic, SLM, Turso, Hub, and enterprise
features with their explicit opt-in and deployment limitations. Detailed
acceptance work remains in [impl.md](impl.md); the deferred-claim inventory is
in [unverified_claims.md](unverified_claims.md).

#### CORE-02: Search & Vector Methods Bypassing `Storage` Trait Interface
- **✅ Fixed (2026-09-13)**: `search_symbols`/`search_vector` moved from inherent `SqliteStorage` methods to `impl Storage for SqliteStorage` overrides; the trait itself now declares both with a default "unsupported" body (mirroring `upsert_trace_span`'s existing shape), so `TursoStorage` and any future backend compile with zero stub work. See §9 Phase 3 row 6 for the full change list and tests. (Note: this does *not* mean `TursoStorage` can safely coexist with `SqliteStorage` in one process — see CORE-01's own section for that separate, unresolved finding. This trait declaration is real and useful independent of that: it also gives `weave-graph-mcp`'s new `weave_search_semantic` tool — §9 Phase 3 row 7 — a way to call `search_vector` through `&dyn Storage` without ever needing a concrete `SqliteStorage` type.)
- **✅ Verified (2026-09-13)**: confirmed — `search_symbols` (`backend.rs:217`) and `search_vector` (`backend.rs:240`) are `pub fn` inherent methods on `SqliteStorage`, not declared anywhere on the `Storage` trait (`weave-graph-core/src/storage.rs:10`). Real gap.
- **Location**: [`crates/weave-graph-core/src/storage.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-core/src/storage.rs) vs [`crates/weave-graph-store-sqlite/src/backend.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-store-sqlite/src/backend.rs)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Rows 30-31) defines Symbol Search (BM25) and Semantic Search (1-bit ANN + int8 rescore) as core search capabilities. However, `search_symbols` (FTS5) and `search_vector` (`vec0`) are defined solely as inherent methods on `SqliteStorage` rather than on the `Storage` trait. As a result, non-SQLite backends (such as `TursoStorage`) cannot fulfill search queries through the storage abstraction.
- **Dual-Tier Impact**:
  - `TursoStorage` cannot support `weave search` or `weave search --semantic` even though libSQL supports full-text search and vector extensions.
  - Test mocks and alternative backends cannot exercise search capabilities.
- **Remediation**:
  - Declare optional `search_symbols` and `search_vector` methods on the `Storage` trait with default "unsupported" error returns.
  - **Feasibility (2026-09-13)**: real and low-risk. `weave-graph-core` already carries `fts`/`vector` Cargo features (used elsewhere in the workspace's feature graph), so gating the new trait methods behind the same features costs nothing new. A default `fn search_symbols(&self, ...) -> Result<_, StorageError> { Err(StorageError::Backend("not supported by this backend".into())) }`-style method needs no new `StorageError` variant at all — `Backend(String)` (`weave-graph-core/src/error.rs`) already fits. Small, additive change. Low effort, no blocker — this is the prerequisite CORE-01's `StorageBackend` enum needs before Turso can answer `weave search` at all.

---

#### CORE-03: MCP Tool Surface Parity Gap Between Standard and Self-Hosted Tiers
- **✅ Fixed (2026-09-13)**: `McpHandler::handle_tools_list`/`handle_tools_call` now register `weave_search_semantic` (feature `vector`) and `weave_policy_lint` (feature `policy-lint`), following the exact `#[cfg(feature = "notes")]` pattern the existing `weave_pin_note`/`weave_recall_notes` pair already used. See §9 Phase 3 row 7 for the full change list and tests.
- **✅ Verified (2026-09-13)**: confirmed — `McpHandler::handle_tools_list` (`crates/weave-graph-mcp/src/handler.rs`) hardcodes exactly 4 tools (6 with `notes`); no `weave_search_semantic` or `weave_policy_lint` tool exists at all, `vector`/`policy-lint` features or not. Real gap. Note the "Self-Hosted Tier bundles vector/policy" framing this cites from `feature_matrix.md` §2 Row 38 is itself imprecise (those are independent Cargo features available in any tier, not exclusive to a "Self-Hosted Tier" as a monolithic switch — see that file's audit note) — but the underlying MCP tool-surface gap is real regardless of that framing.
- **Location**: [`crates/weave-graph-mcp/src/tools.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-mcp/src/tools.rs) & [`crates/weave-graph-mcp/src/handler.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-mcp/src/handler.rs)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Row 38 & §3.3) notes that while Standard Tier exposes 4 baseline tools, Self-Hosted Tier (`weave-custom`) bundles vector and policy linting. However, `McpHandler::list_tools` statically returns only 4 tools (`repo_map`, `file_api`, `trace_calls`, `impact_radius`), lacking conditional compile-time registration (`#[cfg(feature = "vector")]`, `#[cfg(feature = "policy-lint")]`) to expose `weave_search_semantic` and `weave_policy_lint`.
- **Dual-Tier Impact**:
  - When running in Self-Hosted Tier (`weave-custom`), AI agents connected via MCP cannot call `weave_search_semantic` or `weave_policy_lint` even though the binary contains vector and policy engines.
- **Remediation**:
  - Dynamically register `weave_search_semantic` (under `feature = "vector"`) and `weave_policy_lint` (under `feature = "policy-lint"`) in `McpHandler::list_tools`.
  - **Feasibility (2026-09-13)**: real, and the pattern to copy already exists in the same file — `weave_pin_note`/`weave_recall_notes` are already conditionally pushed into the tool list under `#[cfg(feature = "notes")]`. Adding two more tools the same way is mechanical. Two real prerequisites, not blockers but sequencing to note: `weave_search_semantic` should land after SEC-01's masking-order fix (otherwise it just adds a new caller to the same starvation bug), and `weave_policy_lint` needs `weave-graph-mcp` to gain a `policy-lint` Cargo feature and a dependency on `weave_graph_core::policy` (currently absent from that crate's `Cargo.toml`).

---

### 3. Vector & RBAC Interactions (SEC-01 to SEC-04)

#### SEC-01: Post-Query Filtering in Semantic Search (Top-$K$ Starvation & Metadata Leak)
- **✅ Fixed (2026-09-13)**: `vector::search` now filters stage-2 reranked candidates by an optional visibility predicate before `truncate(limit)`, not after — see §9 Phase 2 row 5 for the full change list and tests.
- **✅ Verified (2026-09-13)**: confirmed — `run_semantic` (`search.rs:74-90`) calls `storage.search_vector(&embedder, query, limit, OVERSAMPLE)` (`OVERSAMPLE = 4`, `search.rs:71`) and then filters the returned set through `visible` (`search.rs:86`) *after* retrieval already truncated to `limit` candidates. Masked hits are dropped, never backfilled from beyond the oversample window. Real gap, accurately described.
- **Location**: [`crates/weave-graph-cli/src/search.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-cli/src/search.rs) (`run_semantic`)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Row 31 & §3.3) specifies a 3-stage funnel (1-bit ANN oversample -> int8 rescore -> `RbacGuard` visibility filter). In `search.rs`, `storage.search_vector(&embedder, query, limit, OVERSAMPLE)` performs candidate retrieval unconditionally on the global `vec_chunks` table, returning the top `limit` raw `NodeId`s. The post-query loop filters nodes *after* truncation:
  ```rust
  if visible.is_none_or(|v| v(&node)) {
      nodes.push(node);
  }
  ```
- **Invariant & Security Impact**:
  1. **Top-$K$ Starvation**: If the top $K$ most relevant code chunks belong to internal/restricted files (e.g., `src/payment/secret.rs`), the post-filter discards them all and returns **0 results**, even if visible public symbols exist at ranks $K+1 \dots 2K$.
  2. **Metadata Side-Channel**: An unprivileged user can probe the index with targeted keywords and infer the existence of confidential internal files by detecting unexpected result-count drops or score gaps.
- **Remediation**:
  - Oversample candidate generation in proportion to masked density, or pass an RBAC node-id visibility bitmask into the Stage 2 int8 rescore before truncating to `limit`.
  - **Feasibility (2026-09-13)**: the second option is genuinely the easier one, and more feasible than it first looks. Per `impl.md` M3.7's own build notes, `vec0`'s KNN planner already can't combine a literal `LIMIT` with an `AND rowid IN (...)` filter (a real `sqlite-vec` limitation hit during that milestone), which is *why* Stage 2 (int8 rerank) was already restructured into a plain, non-KNN Rust-side row read and score — not a SQL-level filter. That means a visibility predicate is trivial to thread in: filter candidate rows in that existing Rust loop before taking the top `limit`, no new SQL query shape needed. "Oversample in proportion to masked density" is the harder option (requires estimating density, which nothing currently tracks) and isn't necessary once the simpler fix is in. Low-medium effort, no blocker.

---

#### SEC-02: Unrestricted Code Chunk Embeddings in Shared Database Snapshots
- **✅ Verified (2026-09-13)**: no `[vector.exclude]` (or any per-path exclusion) config exists anywhere in `crates/weave-graph-cli/src/config.rs` or its callers — confirmed by the same `config::get_key` call-site audit done for `docs/product/configuration.md` this session. Real gap: `weave sync push` ships the whole `graph.db` including `vec_chunks`, unconditionally.
- **Location**: [`crates/weave-graph-cli/src/index.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-cli/src/index.rs) (`build_vector_chunks`) & [`crates/weave-graph-store-sqlite/src/vector.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-store-sqlite/src/vector.rs)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Rows 28 & 41) details that `weave index` populates `vec_chunks` and `weave sync push` distributes `.tar.zst` snapshots to Hub. `build_vector_chunks` slices raw source spans for all symbols across the entire repository and stores quantized embeddings in `vec_chunks` without checking `[vector.exclude]` or RBAC visibility rules, embedding proprietary text into shared database artifacts.
- **Invariant & Security Impact**:
  - While node metadata (symbols, paths, line numbers) is masked at query time via `RbacGuard`, the underlying `vec_chunks` table contains mathematical representations of proprietary/private source text.
  - If a snapshot of `.weave/graph.db` is shared across network shares, Turso replicas, or external developers, quantized vectors are susceptible to dictionary attacks and distance probing.
- **Remediation**:
  - Support `[vector.exclude]` path globs in `.weave/config.toml` to prevent embedding private modules.
  - **Feasibility (2026-09-13)**: real and additive — `build_vector_chunks` (`index.rs`) already iterates per-node source spans before embedding, so a path-glob check gates cleanly at that one call site (needs a small glob-matching helper; this workspace has no glob dependency yet, so either add one — `glob`/`globset` — or reuse the existing prefix-matching style `weave_graph_core::policy::in_module` already uses for boundary rules, which may be good enough without full glob syntax). The second half of the remediation ("purge `vec_chunks` rows for non-public paths when exporting/serving shared snapshots") is the harder, separate half: `weave sync push` ships the whole `graph.db` file as-is (no export-time filtering exists anywhere in `sync.rs` today), so this needs new logic, not a toggle on existing filtering. Medium effort overall; the exclude-glob half is low effort, the snapshot-purge half is real new work.
  - When exporting or serving shared snapshots, purge or skip `vec_chunks` rows belonging to non-public paths.

---

#### SEC-03: Storage Trait Abstraction Omission for Vector Search
- **✅ Fixed (2026-09-13)**: landed together with CORE-02 (same change). `search_vector`'s new trait signature takes `Option<&dyn Fn(&Node) -> bool>` for `visible`, exactly as this issue's own feasibility note recommended — not `Identity`, so `weave-graph-core`'s `vector` feature still carries no dependency on its `rbac` feature. See §9 Phase 3 row 6.
- **✅ Verified (2026-09-13)**: same underlying fact as CORE-02 (`search_vector`/`rebuild_vector_index` are inherent `SqliteStorage` methods) — real gap, correctly cross-referenced from a different angle (masking-abstraction consistency rather than backend portability).
- **Location**: [`crates/weave-graph-core/src/storage.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-core/src/storage.rs) vs [`crates/weave-graph-store-sqlite/src/backend.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-store-sqlite/src/backend.rs)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Row 31 & Core Invariant 7) requires that storage and query masking remain unified across backends. However, `search_vector` and `rebuild_vector_index` are implemented as inherent methods on `SqliteStorage`, bypassing the backend-agnostic `Storage` trait and preventing pluggable backends (such as Turso) from implementing vector search.
- **Invariant & Architectural Impact**:
  - Violates **Core Invariant 7** (RBAC must live at the storage/query layer) and crate decoupling.
  - Alternative storage backends (e.g. Turso / libSQL) cannot implement vector search uniformly, preventing pluggable vector storage implementations.
- **Remediation**:
  - Extend the `Storage` trait with optional feature-gated vector methods that accept `Identity` or visibility predicates.
  - **Feasibility (2026-09-13)**: same underlying change as CORE-02's remediation — do them together, not twice. Note `Identity` itself lives behind the `rbac` feature in `weave-graph-core`, while `search_vector` lives behind `vector` — a trait method signature that references `Identity` directly would force `vector` to depend on `rbac` being compiled too, which isn't true today (you can have `vector` without `rbac`). Passing a plain `Option<&dyn Fn(&Node) -> bool>` visibility predicate (the same shape every other masked call site in this codebase already uses) avoids that coupling — prefer that over threading `Identity` into the trait itself.

---

#### SEC-04: MCP Server Omission of Vector Search Tool
- **✅ Fixed (2026-09-13)**: landed together with CORE-03 (same change). `weave_search_semantic` (`crates/weave-graph-mcp/src/search_semantic.rs`) is wrapped with the session's bound `RbacGuard` exactly as this issue's remediation asked — same `self.rbac_guard.as_ref().map(|g| move |n: &Node| g.visible(n))` pattern every other masked tool in this handler already uses, and it lands *after* SEC-01's pre-truncation fix (Phase 2), so it never shipped the Top-K starvation bug. See §9 Phase 3 row 7.
- **✅ Verified (2026-09-13)**: same underlying fact as CORE-03, restated for the vector-specific case. Real gap.
- **Location**: [`crates/weave-graph-mcp/src/tools.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-mcp/src/tools.rs) & [`crates/weave-graph-mcp/src/handler.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-mcp/src/handler.rs)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Rows 31 & 38) defines Semantic Search as an accessible intelligence capability. However, `crates/weave-graph-mcp` does not declare a `weave_search_semantic` tool schema or dispatch handler in `McpHandler::call_tool`, leaving AI agents unable to trigger vector search over MCP even when the binary is compiled with `--features vector`.
- **Impact**:
  - LLM agents interacting via MCP are limited to exact-match or graph-walking tools, unable to leverage semantic code discovery.
- **Remediation**:
  - Add `weave_search_semantic` to MCP tool handlers under `#[cfg(feature = "vector")]`, ensuring it is wrapped with the session's `RbacGuard`.
  - **Feasibility (2026-09-13)**: same remediation as CORE-03's tool-registration half; see that note. Do CORE-03 and SEC-04 as one change, not two.

---

### 4. CLI Permission Execution & Identity Gaps (SEC-05 to SEC-07)

#### SEC-05: Opt-In RBAC Masking Loophole in CLI Commands
- **✅ Fixed (2026-09-13, narrower form)**: `weave serve --mcp` gained `--require-as` and `[rbac] require_identity` (`.weave/config.toml`) — when either is set and `--as` is omitted, the server refuses to start (`crates/weave-graph-cli/src/main.rs::cmd_serve`). This is the narrower recommendation below, not the literal "always construct a guard" remediation (which would have reproduced the M3.0 regression this issue's own Verified note describes) — every other RBAC-gated command's existing "no `--as` == unmasked" behavior is untouched. See §9 Phase 4 row 8.
- **⚠️ Verified (2026-09-13) — real behavior, but reconsider "Critical"/"loophole" framing**: the mechanism is accurately described (`as_subject: None` → `guard`/`mask` both `None` → full unmasked access). But `docs/impl.md`'s M3.0 entry documents this as the *deliberate, tested* fix, not a defect: an earlier build masked the anonymous identity by default, which broke every existing command the moment `rbac` was compiled in — regardless of whether anyone had opted into enforcement — and was reverted on purpose ("masking only engages when `--as <subject>` is actually supplied; an omitted `--as` runs byte-identical to a `not(feature = "rbac")` build"), with a named regression test (`test_cli_query_export_report_and_reindex_fast_path`) guarding exactly this behavior. For a human running `weave` locally against their own checkout, "no `--as` == full access" is correct (they already have raw filesystem access to every symbol; masking would add no real boundary). The real, narrower risk this issue is pointing at: an operator standing up a **shared** `weave serve --mcp` or CI runner for multiple identities who forgets to pass `--as` gets an unmasked session — that's an operational/deployment footgun worth documenting prominently, not a code loophole to patch by inverting the default (inverting it would reproduce the exact regression M3.0 already fixed once). Recommend re-titling/re-scoping this issue as a deployment-hardening doc gap rather than a "Critical" code defect, unless the remediation is something narrower than "always construct a guard" (e.g., a `weave serve --mcp` startup warning when `--as` is omitted and `rbac` is compiled in, which doesn't touch every other RBAC-gated command's existing, tested, intentional behavior).
- **Location**: [`crates/weave-graph-cli/src/main.rs:L1304-1391`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-cli/src/main.rs#L1304-L1391) (`cmd_query`, `cmd_report`, `cmd_export`)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Row 29, Row 143, & §3.1) establishes that Self-Hosted Tier (`weave-custom`) runs under mandatory query-layer RBAC masking. In `main.rs`, RBAC masking is wired via:
  ```rust
  #[cfg(feature = "rbac")]
  let guard = as_subject.map(|s| rbac::guard_for(root, Some(s)));
  let mask = guard.as_ref().map(|g| |n: &Node| g.mask_node(n));
  ```
  When a caller invokes `weave query`, `weave report`, or `weave export` without the `--as` flag, `as_subject` is `None`, evaluating `guard` and `mask` to `None`. This creates an inverted "opt-in" security model where omitting `--as` grants full unmasked admin access to all internal symbols instead of resolving to `Identity::anonymous()`.
- **Security Impact**:
  - In **Self-Hosted Tier** / Custom Mode deployments, an unauthenticated user or script gets **unmasked full admin access by simply omitting `--as`**.
  - To be secure by default, omitting `--as` must resolve to the `anonymous` identity (public contract only), rather than granting complete internal visibility.
- **Remediation**:
  - In Custom Mode builds, construct `guard_for(root, as_subject.as_deref())` unconditionally. `None` correctly resolves to `Identity::anonymous()`.
  - **Feasibility (2026-09-13) — not recommended as written**: mechanically trivial (delete the `Option`-wrapping), but doing it exactly as proposed reintroduces the precise regression `impl.md` M3.0 already fixed once (a build with `rbac` compiled would start masking `weave query`/`report`/`export` by default the moment the feature is on, breaking every existing non-`--as` invocation — the opposite of Core Invariant 8's feature-isolation requirement). "Custom Mode builds" isn't a runtime condition this code can branch on either — `--features custom` only affects what's *compiled*, not a flag any command sees at runtime, so "in Custom Mode builds, construct unconditionally" isn't actually expressible as written without a second runtime toggle. A narrower, real fix: add an opt-in `[rbac] require_identity = true` config key (or a `weave serve --mcp`-specific `--require-as` flag) that refuses to start/run without `--as` when set — gives operators of shared/multi-tenant deployments a way to close this, without changing the default for every other `rbac`-compiled repo. Low-medium effort for that narrower version.

---

#### SEC-06: Waiver Authorization Bypass via `--as` Omission
- **✅ Fixed (2026-09-13, narrower form)**: `waiver::authorize` (`crates/weave-graph-cli/src/waiver.rs`) now rejects an anonymous waiver (`as_subject: None`) only when this repo's own `[rbac.users]` config actually grants the `"allow-drift"` role to at least one subject — read via the existing `config::read_rbac_users` helper, no new config surface. A repo that never configured `allow-drift` for anyone keeps today's behavior byte-for-byte. Tests: `waiver::tests::rbac_gated::authorize_accepts_anonymous_waiver_when_config_lacks_allow_drift`, `authorize_rejects_anonymous_waiver_when_config_grants_allow_drift`. See §9 Phase 4 row 8.
- **⚠️ Verified (2026-09-13) — same design tradeoff as SEC-05, by explicit intent**: `waiver.rs::authorize` was written this session (`impl.md` M3.10) specifically mirroring M3.0's own precedent: "a no-op whenever `rbac` isn't compiled in, or `--as` was never given... following M3.0's own feature-isolation precedent exactly." This was a deliberate choice, not an oversight — the alternative (waivers always require `rbac` + an authorized `--as`, even in builds/repos that never opted into RBAC at all) would make `--allow-drift`/`--skip` silently start requiring an identity the moment `rbac` is compiled in, for every consumer of this CLI, which is the exact kind of surprise regression M3.0's own history warns against. The real risk is narrower and correctly named in the Impact bullet: a repo that *has* configured `[rbac.users]` with role-gated waivers, and expects that to be enforced, must remember to invoke `weave check-contracts --allow-drift --as <subject>` (not bare `--allow-drift`) in every CI job — that's a real footgun worth a prominent doc callout (e.g., in `self-hosted.md`'s waiver section) rather than a code defect. If the intent is genuinely "once any role is configured for `allow-drift` anywhere in this repo's config, bare `--allow-drift` must always be rejected," that's a real, narrow, implementable change — distinct from "reject all identity-less waivers unconditionally," which would break every non-RBAC repo's existing `--allow-drift` usage.
- **Location**: [`crates/weave-graph-cli/src/waiver.rs:L21-35`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-cli/src/waiver.rs#L21-L35)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Row 34, Row 144, & §3.2) specifies that contract waivers in Self-Hosted Tier are strictly authorized only if `--as <subject>` possesses the `"allow-drift"` role. However, `waiver::authorize` explicitly returns `Ok(())` if `as_subject` is `None`:
  ```rust
  #[cfg(feature = "rbac")]
  pub(crate) fn authorize(root: &Path, as_subject: Option<&str>) -> Result<(), String> {
      let Some(subject) = as_subject else {
          return Ok(()); // <--- Bypasses waiver gate!
      };
      let guard = crate::rbac::guard_for(root, Some(subject));
      if guard.can_waive() { Ok(()) } else { Err(...) }
  }
  ```
  This causes unauthenticated CLI invocations (`weave check-contracts --allow-drift`) to completely bypass role verification.
- **Security Impact**:
  - An unauthorized contractor (`carol = ["contractor"]`) who is blocked by `weave check-contracts --allow-drift --as carol` can simply run `weave check-contracts --allow-drift` (omitting `--as`) and the waiver check passes unrestricted!
- **Remediation**:
  - When `rbac` is active, waiving a CI gate must require an explicit authorized subject, rejecting anonymous waivers.
  - **Feasibility (2026-09-13) — not recommended exactly as written, same reasoning as SEC-05**: "when `rbac` is active" isn't a runtime signal — plenty of `--features custom` repos never populate `[rbac.users]` with an `"allow-drift"` role at all and use `--allow-drift` exactly as a non-rbac repo would; rejecting every identity-less waiver purely because the feature is compiled would break that population for no security gain. A real, narrower version: reject an anonymous waiver only when *this repo's own config* actually grants `"allow-drift"` to anyone (i.e., `[rbac.users]` has at least one subject holding that role) — a real, checkable signal that this repo opted into role-gated waivers, using config the CLI already parses. Low-medium effort; unlike the literal text, doesn't regress every `rbac`-compiled-but-unconfigured repo's existing `--allow-drift` usage.

---

#### SEC-07: MCP Stdio Transport Lacks Per-Request Authentication
- **✅ Verified (2026-09-13)**: confirmed — `McpHandler::with_identity` binds one `RbacGuard` for the process's entire lifetime (`handler.rs`), called once at `cmd_serve` startup; nothing in `handle_message`/`handle_tools_call` inspects a per-call identity. Real, and an accurate statement of the one-process-one-identity model this handler deliberately uses (matches the CLI's own "one session, one identity" precedent) — genuinely a gap only for a *multi-tenant proxy in front of one `weave serve` process*, which isn't this server's stated design point.
- **Location**: [`crates/weave-graph-cli/src/main.rs:L1474-1480`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-cli/src/main.rs#L1474-L1480) & [`crates/weave-graph-mcp/src/handler.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-mcp/src/handler.rs)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Row 38) states that in Self-Hosted Tier, the MCP server masks responses through the session identity. However, `weave serve --mcp` accepts a single `--as <subject>` flag at process launch and initializes a static `RbacGuard`. The MCP stdio/HTTP protocol handler does not inspect per-request JSON-RPC headers or metadata, locking the entire server instance to a single caller identity.
- **Impact**:
  - Multi-tenant IDE plugins or proxy servers cannot forward client credentials per-tool call.
- **Remediation**:
  - Support client credential metadata in MCP JSON-RPC headers or request params.
  - **Feasibility (2026-09-13)**: real for the HTTP transport (a custom header per request is easy to add and read before dispatching), but the stdio transport (the primary, most-used one — Claude Desktop/Code, Cursor, etc. all launch `weave serve --mcp` over stdio) has no header concept at all; MCP's own JSON-RPC envelope has no standard per-call identity field either. A per-request identity would need either a non-standard extension to every `tools/call` request's `arguments` (invasive, and every MCP client would need to know to send it) or a session-establishment handshake beyond what `initialize` currently does. Medium-high effort, and the stdio half genuinely requires protocol-level design work, not just wiring.

---

### 5. SSO / SCIM & Role Mapping Gaps (IDP-01 to IDP-02 & RBAC-01 to RBAC-02)

#### IDP-01: Non-Standard SCIM 2.0 User Attribute & Role Ingestion
- **✅ Fixed (2026-09-13)** (`roles` half only, as scoped — `groups` deferred to RBAC-02): `provision_request` now accepts both the flat-string and RFC 7643 object-array shapes. See §9 Phase 2 row 4.
- **✅ Verified (2026-09-13) — real gap, but the exact failure mode is different from what's written**: read `provision_request` (`rbac.rs:302-318`) directly. `roles` is extracted via `json.get("roles").and_then(|v| v.as_array())` — an RFC 7643 array-of-objects payload **is** a JSON array, so `.as_array()` still succeeds; the inner `.filter_map(|r| r.as_str()...)` then silently drops every element (objects aren't strings), producing an **empty `Vec<String>`**. The `vec!["reader"]` fallback (`.unwrap_or_else`) only fires when the `roles` key is *missing entirely* — it is never reached for a present-but-wrongly-shaped array. Net effect is the same in practice (an identity with no meaningful roles), but "defaults to unprivileged `reader`" should read "silently resolves to zero roles" — there's no `"reader"` string anywhere in the actual outcome for this input shape. `groups` isn't parsed by `provision_request` at all (no such field exists on `DirectoryMutation::Provision`), so the doc's `"groups"` JSON example is aspirational, not a parsing target today.
- **Location**: [`crates/weave-graph-cli/src/rbac.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-cli/src/rbac.rs) (`provision_request`)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Row 44) specifies that `weave rbac serve-scim` ingests user and group provisioning from enterprise IdPs (Okta, Azure AD, Google Workspace). However, `provision_request` deserializes `roles` and `groups` assuming a non-standard flat JSON string array (`["internal"]`). Enterprise IdPs conforming to RFC 7643 send complex object arrays (`[{"value": "internal", "primary": true}]`), causing JSON deserialization errors and defaulting synced users to unprivileged `reader`.
- **Impact**:
  - Standard RFC 7643 SCIM 2.0 payloads from Okta, Azure AD, and Google Workspace send complex object arrays:
    ```json
    "roles": [{"value": "internal", "primary": true}],
    "groups": [{"value": "grp-1", "display": "engineering-leads"}]
    ```
  - Flat string extraction fails on objects, causing enterprise directory syncs to fall back to unprivileged `vec!["reader"]`.
- **Remediation**:
  - Accept both flat string arrays and standard RFC 7643 object arrays for `roles` and `groups`, extracting `.value` and `.display`.
  - **Feasibility (2026-09-13)**: the `roles` half is real and low effort — `provision_request` already works with a raw `serde_json::Value`, so the array-element match just needs to try `.as_str()` first, then fall back to `.get("value").and_then(Value::as_str)` for an object shape, instead of the current `filter_map` that only tries the string case. The `groups` half is a bigger claim than it looks: `DirectoryMutation::Provision` has no `groups` field today at all (confirmed: `rbac.rs`), so "extracting `.display`" needs a new field end-to-end — the struct, the `.weave/rbac-directory.toml` serialization, and something downstream that actually *uses* a group (which doesn't exist yet — see RBAC-02, this is the same gap from the ingestion side). Do the `roles` fix alone first; treat `groups` as RBAC-02's dependency, not a one-line addition here.

---

#### IDP-02: Unauthenticated Loopback SCIM Server
- **✅ Fixed (2026-09-13)**: `ScimServer`/`weave rbac serve-scim` now support an optional `[rbac.scim] token` requiring `Authorization: Bearer` on every request. See §9 Phase 1 row 2. (The "zero hits" grep in the verification note below predates this fix, from earlier the same day.)
- **✅ Verified (2026-09-13)**: confirmed, and this session's own `docs/product/self-hosted.md` fix independently reached the same conclusion while correcting a fictional "validates bearer tokens" claim there — `grep -n "bearer\|Bearer\|Authorization" crates/weave-graph-cli/src/rbac.rs` returns zero hits. This is real, and (same as HUB-02) a deliberate v1 design choice per `impl.md` M3.4 ("loopback only... trusted network"), not an oversight — worth deciding explicitly whether that trust model is still acceptable now that SCIM can elevate a subject to `"internal"`, rather than treating it as an unnoticed bug.
- **Location**: [`crates/weave-graph-cli/src/rbac.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-cli/src/rbac.rs) (`ScimServer`)
- **Root Cause**:
  `docs/feature_matrix.md` (§1 & §2, Row 44) specifies SCIM directory synchronization for secure user management. However, `ScimServer` binds a loopback HTTP socket and processes mutation endpoints (`POST /Users`, `POST /sync`) without validating an `Authorization: Bearer <token>` header or shared secret, allowing unauthenticated local processes to inject role elevations into `.weave/rbac-directory.toml`.
- **Impact**:
  - Any local unprivileged process or multi-tenant container sharing the host network namespace can issue provisioning mutations and elevate its subject to `"internal"`.
- **Remediation**:
  - Add optional `bearer_token` configuration in `.weave/config.toml` (`[rbac.scim] token = "..."`).
  - **Feasibility (2026-09-13)**: real, small-to-medium effort. Correction to scope: the SCIM server's `read_request` (`rbac.rs:322`) currently discards headers entirely (splits only method/path/body from the raw request) — it does not already parse individual headers the way `weave-graph-hub`'s `server.rs::header()` does. Adding bearer-token support means first giving `read_request` a header lookup (a small, bounded addition modeled on that exact existing hub helper), then checking it before dispatch. No architectural blocker, just slightly more than "one more header check."

---

#### RBAC-01: Binary Role Model Collapse
- **✅ Verified (2026-09-13) — accurate, and now the documented reality everywhere else too**: `Identity::is_internal`/`Identity::can_waive` (`weave-graph-core/src/rbac.rs`) are the only two role strings this engine reads differently from any other; this was independently confirmed and is now stated plainly across `docs/product/{configuration,self-hosted,features,cli-reference}.md` after this session's doc-accuracy pass (they previously described a fictional per-role `mask = [...]` permission system). Whether this is a "gap" to fix or the intended minimal design is a real product decision, not a bug in the code matching its own docs — but the current docs and this issue now agree on what the code actually does.
- **Location**: [`crates/weave-graph-core/src/rbac.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-core/src/rbac.rs) (`RbacGuard::visible`)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Row 29 & §3.1) establishes that `RbacGuard` manages visibility across distinct roles. However, `RbacGuard::visible` implements a binary evaluation: `self.identity.is_internal() || (self.is_public)(node)`. The engine only recognizes `"internal"` and `"allow-drift"`, collapsing all custom roles (`contractor`, `billing-team`, `security-auditor`) into unprivileged `anonymous` visibility without path-prefix scoping.
- **Impact**:
  - Only two strings (`"internal"` and `"allow-drift"`) have semantic meaning in the engine.
  - Granular enterprise roles (e.g. `billing-team`, `security-auditor`, `contractor`) are completely ignored and treated identically to unauthenticated `anonymous` users.
  - Path-scoped access control (e.g., granting a team read access to `src/payments/**` while masking `src/auth/**`) is impossible.
- **Remediation**:
  - Support path-prefix role mapping in `[rbac.roles]` (e.g. `billing = ["src/billing/**", "src/shared/**"]`).
  - **Feasibility (2026-09-13)**: real, but bigger than a config addition — it's a design change to the masking primitive itself. `RbacGuard::new(identity, is_public)` takes one `is_public` closure shared by every non-`"internal"` identity; per-role path scoping means `is_public` would need to depend on *which* role(s) the specific identity holds, not just be a fixed function of the node. That's a real API change to `RbacGuard`/`guard_for`, touching every call site that builds a guard (`query`, `report`, `export`, `search`, `serve --mcp`, `policy lint`/`drift`, `blast`/`check-contracts`'s waiver check). Medium-high effort, genuine design work, not just wiring a new config table. Also note: this is a repeat of the exact simplification `feature_matrix.md`'s original (now-corrected) fictional `[rbac.roles.*].mask = [...]` table implied existed — worth deciding deliberately whether to actually build this, rather than re-adding it because a doc once claimed it was already there.

---

#### RBAC-02: Lack of Enterprise Group-to-Capability Mapping
- **✅ Verified (2026-09-13)**: confirmed — `read_rbac_users` (`config.rs`) only ever reads `[rbac.users]`'s flat `subject = ["role", ...]` table; no `[rbac.group_mappings]` or equivalent exists anywhere in `config.rs` or its callers. Real gap.
- **Location**: [`crates/weave-graph-cli/src/config.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-cli/src/config.rs) (`read_rbac_users`)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Row 44 & Row 143) notes that SCIM synchronizes IdP user/group memberships into `.weave/rbac-directory.toml`. However, `read_rbac_users` in `config.rs` only parses direct 1:1 `username = ["role"]` tables under `[rbac.users]`. There is no group-mapping translation layer (`[rbac.group_mappings]`) to map enterprise directory groups (e.g. `CN=Architecture-Review-Board`) to Weave roles (`allow-drift`, `internal`).
- **Impact**:
  - Directory groups synced from corporate IdPs (e.g., `CN=Architecture-Review-Board`) cannot be mapped to Weave capabilities (`allow-drift`) without manual username overrides.
- **Remediation**:
  - Introduce `[rbac.group_mappings]` table in `.weave/config.toml`.
  - **Feasibility (2026-09-13)**: real, low-medium effort, but depends on IDP-01's `groups` parsing landing first — there's no group data flowing into `.weave/rbac-directory.toml` at all today for a mapping table to translate. Once `groups` exists on the ingested identity, a `[rbac.group_mappings]` table read by `config.rs` (same shape as the existing `read_rbac_users`) and applied as a lookup during `guard_for`/directory resolution is straightforward. Sequence after IDP-01, not before.

---

### 6. Policy-Lint, Provenance & Hub Gaps (POL-01 to POL-05 & PROV-01, FED-01, HUB-01 to HUB-03)

#### POL-01: Masked CI Policy Linting Yields False Negatives
- **✅ Verified (2026-09-13) — real, mechanism confirmed; remediation cites a flag that doesn't exist**: read `visible_view` (`policy.rs:197-224`) directly — it drops hidden nodes AND any edge touching one from *both* endpoints' visible-id set, so a `disallow` rule can never see an edge crossing into a masked module; `cmd_policy_lint` then genuinely reports "✓ no boundary violations" in that case. Confirmed real. However, the proposed remediation ("treat `skipped_edges > 0` as inconclusive when `--strict` is passed") references a `--strict` flag on `weave policy lint` that does not exist — today `weave policy lint` always exits non-zero on any violation and has no severity-mode flag at all (confirmed: zero `--strict` anywhere in `main.rs`'s `Commands::PolicyAction` or `policy.rs`). Rephrase the remediation as "add a mode/flag" (net-new), not "when `--strict` is passed" (implying one already exists).
- **Location**: [`crates/weave-graph-cli/src/policy.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-cli/src/policy.rs) (`cmd_policy_lint`)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Row 35 & §3.4) specifies that Self-Hosted Tier boundary linting evaluates policy rules over the visible graph while tracking skipped counts. However, when running `weave policy lint --as <contractor>`, `cmd_policy_lint` filters edges through `RbacGuard` before checking `BoundaryRule::Disallow` / `Require`. Edges connecting to or from hidden nodes are skipped:
  ```rust
  if view.hidden_nodes > 0 || view.skipped_edges > 0 {
      println!("  {} edge(s) and {} symbol(s) skipped (rbac-masked)", view.skipped_edges, view.hidden_nodes);
  }
  ```
  Because the CLI exits with code 0 (`✓ no boundary violations`) despite skipped edges, illegal cross-boundary dependencies involving private components pass CI silently unless `--strict` is enforced.
- **Security & Integrity Impact**:
  - If a prohibited cross-boundary edge exists between a public module and a private module (e.g., `src/ui/widget.rs -> src/billing/secret.rs`), a restricted identity sees **0 violations** and a green checkmark (`✓ no boundary violations`).
  - Running policy gates under restricted service accounts produces false compliance verification.
- **Remediation**:
  - Require an `internal` role for CI policy gating, or treat `skipped_edges > 0` as an inconclusive/warning exit code when `--strict` is passed.
  - **Feasibility (2026-09-13)**: the second option is real and low effort once the wording is fixed — there is no existing `--strict` flag to "pass" (see this issue's own Verified note), so the actual work is *adding* a new flag (e.g. `--fail-on-masked`) to `PolicyAction::Lint` and checking `view.skipped_edges > 0` under it in `cmd_policy_lint` — both values are already computed and available at that call site (`policy.rs`), just not currently acted on. The first option ("require `internal` role") is simpler code but a bigger behavioral demand — it would make `weave policy lint` refuse to run at all for any non-internal identity, foreclosing the "advisory scan by a restricted service account" use case entirely rather than just flagging it as incomplete.

---

#### POL-02: Structural Policy Boundaries Blind to High Semantic Coupling
- **✅ Verified (2026-09-13)**: confirmed — `BoundaryRule`/`lint` (`weave-graph-core/src/policy.rs`) only ever inspect `Edge` records (real AST call/import/reference edges); there is no code path from `policy.rs` into `vec_chunks` or any embedding distance computation. Real, accurately-scoped gap (a genuinely new capability, not a bug).
- **Location**: [`crates/weave-graph-core/src/policy.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-core/src/policy.rs) (`lint`)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Row 35 & Row 31) defines both boundary linting and vector embedding capabilities in the Self-Hosted Tier. However, `BoundaryRule::Disallow` in `policy.rs` checks only explicit syntactic AST references (`Edge::kind` = call, import, ref). It does not interface with the vector embeddings in `vec_chunks` to detect semantic drift or high conceptual coupling across forbidden architectural boundaries.
- **Architectural Impact**:
  - If two modules are forbidden from depending on one another, but duplicate logic, shared schema conventions, or dynamic string reflection bridge the modules, syntactic linting reports clean compliance.
- **Remediation**:
  - Add optional semantic drift checks: flag pairs of symbols across `Disallow` boundaries whose vector cosine distance falls below a similarity threshold (e.g. `similarity > 0.88`).
  - **Feasibility (2026-09-13)**: real, but a genuinely new cross-feature integration, not a small addition — `weave-graph-core::policy` (feature `policy-lint`) has no dependency on `vector`-feature types (`vec_chunks`, embeddings) today, and `weave-graph-cli::policy.rs` doesn't open the vector store at all. This needs `policy-lint` to gain an optional dependency on `vector`-feature machinery (mutual feature-gating both ways, similar to how `vector` already depends on `fts`), plus a real decision on where the O(boundary-pairs × chunk-pairs) cosine comparison runs without becoming a CI-time cost blowup on large repos. Medium-high effort, real design work.

---

#### POL-03: RBAC-Masked Views Skew Cycle and Orphan File Drift Signals
- **✅ Fixed (2026-09-13)**: `cmd_policy_drift` now annotates orphans that only appear because masking severed their real inbound edges. See §9 Phase 2 row 3.
- **✅ Verified (2026-09-13)**: same `visible_view` mechanism confirmed for POL-01 applies identically to `cmd_policy_drift` (it calls the same `visible_view` helper before `find_cycles`/`orphan_files`) — a real public utility whose only callers sit in a masked module will show 0 visible inbound edges to a non-`internal` identity, exactly as described. Real gap. Note `weave policy drift` is always advisory (never fails CI, no exit-code implication either way) — this affects the *signal's accuracy*, not a false CI pass, which is a materially smaller-severity version of the risk POL-01 describes.
- **Location**: [`crates/weave-graph-core/src/policy.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-core/src/policy.rs) (`find_cycles`, `orphan_files`)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Row 36) states that Architecture Drift (`weave policy drift`) computes Tarjan SCC cycles and orphan files over the visible subgraph. In `policy.rs`, `find_cycles` and `orphan_files` execute after `RbacGuard` pruning. Severing masked internal edges breaks closed cycle paths and leaves public symbols with only private callers showing 0 in-degree, generating false orphan alerts.
- **Impact**:
  1. **Cycle Masking**: An architectural cycle that passes through an internal module is severed when viewed by an external role, hiding architectural debt.
  2. **False Orphan Alerts**: A public utility file whose sole callers are inside private modules will report zero inbound edges, falsely classifying it as an orphan file.
- **Remediation**:
  - Annotate orphan reports with `(has hidden inbound edges)` when masked edges target the file.
  - **Feasibility (2026-09-13)**: real and genuinely low effort — `cmd_policy_drift` already has both the masked `view` and (via a second, unmasked `visible_view(&storage, None)` call) the full graph available in the same function; diffing "orphan in the masked view but not in the unmasked view" is a small addition to `policy.rs`, no change needed to `weave_graph_core::policy::orphan_files` itself. Cheapest real fix in this whole register.

---

#### POL-04: Architectural Policies Lack Role Ownership & Role Exemptions
- **✅ Verified (2026-09-13)**: confirmed — `BoundaryRule`/`Boundary` (`weave-graph-core/src/policy.rs`) have exactly two fields, `from`/`to` path strings; no `owner_role`/`allowed_roles` field exists on any policy type, nor in the YAML parser (`policy.rs`'s `RuleEntry`/`BoundaryYaml` in the CLI crate). Real, net-new capability gap.
- **Location**: [`crates/weave-graph-core/src/policy.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-core/src/policy.rs) (`BoundaryRule`) & [policy.rs](file:///Users/ragu/Code/weave-graph/crates/weave-graph-cli/src/policy.rs)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Row 35 & §3.4) specifies role-aware boundary evaluation. However, `BoundaryRule` in `crates/weave-graph-core/src/policy.rs` is defined strictly via path string prefixes (`from: "src/ui", to: "src/db"`). It lacks fields for `allowed_roles` or `owner_role`, preventing teams from declaring authorized cross-boundary exceptions for specific engineering roles.
- **Impact**:
  - Policy engine cannot enforce module ownership (e.g., `owner_role: "core-infrastructure"`).
  - Cannot specify role-based boundary exemptions (e.g., disallow `ui -> db` except for developers with `data-engineer` role).
- **Remediation**:
  - Extend policy schema to support `allowed_roles` and `owner_role` attributes on boundary rules.
  - **Feasibility (2026-09-13)**: real, low-medium effort — simpler than it first looks, since `allowed_roles`/`owner_role` describe the *caller's* identity (the `--as <subject>` running `weave policy lint`), not per-edge code authorship. The schema extension (`BoundaryYaml`/`RuleEntry` in `weave-graph-cli::policy.rs`, `BoundaryRule`/`Boundary` in `weave-graph-core::policy.rs`) is additive and `#[serde(default)]`-backward-compatible. Evaluation just needs `cmd_policy_lint` to check the bound identity's roles against a rule's `allowed_roles` before counting its violations — the same "identity already resolved, check its roles" shape `waiver::authorize`/`can_waive` already use, not new data the graph model lacks.

---

#### POL-05: Waiver Role Hierarchy & Environment Bypass Audit Gaps
- **✅ Verified (2026-09-13) — real behavior, but remediation names a nonexistent env var**: confirmed — `require_reason` (`waiver.rs`) is only invoked on the CLI-flag path (`--allow-drift`/`--allow-drift-for`/`--skip`); the `WEAVE_SKIP_CONTRACTS`/`WEAVE_SKIP_BLAST`/`WEAVE_ALLOW_DRIFT_REPOS` env-var paths never call it. This is stated as a deliberate scope line in `impl.md` M3.10 ("env-var-triggered waivers don't need one — the env var itself is the audit trail"), not an oversight, but the tradeoff is real and worth this issue's scrutiny. Two corrections to the specifics: the actual env var is `WEAVE_ALLOW_DRIFT_REPOS` (a repo allow-list), not a boolean `WEAVE_ALLOW_DRIFT`; and there is no `WEAVE_WAIVER_REASON` env var anywhere in the code — `require_reason` only ever reads a CLI `--reason` value. If mandatory-reason-for-env-var-waivers is wanted, it needs a new env var, not wiring an existing one.
- **Location**: [`crates/weave-graph-cli/src/waiver.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-cli/src/waiver.rs) (`authorize`, `require_reason`)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Row 34, Rows 144, 145, 148) specifies that waivers require both the `allow-drift` role and an explicit reason, whether invoked via CLI flags (`--reason`) or environment variables (`WEAVE_WAIVER_REASON`). In `waiver.rs`, `require_reason` is only executed when parsing the `--allow-drift` CLI flag. When `WEAVE_ALLOW_DRIFT=1` is set, the CLI skips reason validation, omitting audit trails in automated CI environments.
- **Impact**:
  - CI runners using environment variables leave no structured audit reason in markdown waiver notices.
- **Remediation**:
  - Require `WEAVE_WAIVER_REASON` when `WEAVE_ALLOW_DRIFT` is set.
  - **Feasibility (2026-09-13)**: as written this references two env vars that don't exist (see this issue's own Verified note — the real ones are `WEAVE_SKIP_CONTRACTS`/`WEAVE_ALLOW_DRIFT_REPOS`, and there's no `WEAVE_WAIVER_REASON` anywhere). The real version — reading an env var for the reason text and requiring it alongside `WEAVE_ALLOW_DRIFT_REPOS`/`WEAVE_SKIP_CONTRACTS`/`WEAVE_SKIP_BLAST` — is mechanically trivial (`waiver.rs` already has `require_reason`, this just needs a second call site for the env-var path). But it's a deliberate reversal of `impl.md` M3.10's stated design ("env-var-triggered waivers don't need one — the env var itself is the audit trail"), not a bug fix — decide explicitly whether that tradeoff should change before implementing, since M3.10's own tests currently assert the *opposite* (`WEAVE_SKIP_BLAST` succeeding with no reason).

---

#### FED-01: Cross-Repo Boundary Rules Blind in Multiple Mode
- **✅ Verified (2026-09-13)**: confirmed — `cmd_policy_lint`/`cmd_policy_drift` (`policy.rs`) both call only `crate::open_storage_for_read(root)`, the single local repo's own storage; no `federation::open_federated_storage` call or `[federation] linked_repos` read anywhere in `policy.rs`. Real gap, net-new scope (not a regression).
- **Location**: [`crates/weave-graph-cli/src/policy.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-cli/src/policy.rs) (`cmd_policy_lint`)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Row 35 & §4) describes Multiple Mode as federating sibling directories configured in `[federation] linked_repos`. However, `cmd_policy_lint` in `policy.rs` opens only the single local repository database via `open_storage_for_read(root)`, ignoring linked repository graphs and cross-repo monikers during policy evaluation.
- **Mode & Architectural Impact**:
  - In **Multiple Mode** (`mode = "multiple"`), teams cannot declare architectural boundaries across federated repositories (e.g., forbidding `frontend-repo/src/views` from calling `billing-repo/src/internal_db`).
- **Remediation**:
  - Allow `policy.yaml` to specify repo-scoped boundary endpoints (`from: "repo-a/src/views", to: "repo-b/src/db"`) and evaluate against the fused federated graph.
  - **Feasibility (2026-09-13)**: real, medium effort, and grounded in existing primitives rather than starting from zero — `federation::open_federated_storage`/`repo_contract_map` already produce a composite graph with repo-labeled nodes and `CROSS_REPO` edges (built for `weave check-contracts --scoped` and `weave plan-migration`). `cmd_policy_lint`/`cmd_policy_drift` would need a new `--federated` path that opens that composite instead of `open_storage_for_read(root)`, and `BoundaryYaml`'s `from`/`to` parsing needs a `repo/path` prefix convention (`in_module`'s prefix-match logic already generalizes to this once nodes carry a repo-qualified path). Real work, but the federation substrate this needs already exists and is tested — not a from-scratch feature.

---

#### PROV-01: Snapshot Provenance Verifier Unwired in Hub Registry
- **✅ Fixed (2026-09-13)**: the design decision this issue's own feasibility note called for — "a registry-side config knob for a deployment-supplied verifier/key" — is made and implemented: `Registry::with_provenance_verifier(Arc<dyn SnapshotProvenanceVerifier>)` (new builder method, `crates/weave-graph-hub/src/registry.rs`) lets a deployment bind its own secret via `MockSnapshotProvenanceVerifier::with_key(<secret>)` (never the mock's own well-known default key — `with_key` already existed for exactly this). `push_complete` now verifies the spooled payload against the configured verifier *before* the head/rate-limit state advances (new `PushDecision::SignatureInvalid`, mapped to HTTP `400` in `server.rs`); a signature is hex-encoded bytes over the wire (`decode_hex`), matching the raw `Vec<u8>` `sign_snapshot` already returns. `weave-registry` gained a new `--provenance-key <secret-u64>` flag (mirroring `--auth-token`'s HUB-02 precedent exactly) — omitting it keeps every push unverified, byte-identical to before. Tests: 7 in `registry::tests::provenance_gated` (correct signature accepted; missing/wrong-key/wrong-payload/malformed-hex signatures rejected; a rejected push never advances the head or commits) plus 2 real-TCP tests in `server::tests` (`push_with_a_missing_signature_returns_400_when_a_verifier_is_configured`, `push_with_a_valid_signature_is_accepted_when_a_verifier_is_configured`) and 1 in `weave-registry`'s own `tests::parse_args_accepts_an_optional_provenance_key`. Default (no `--provenance-key`) behavior confirmed unchanged: the full `weave-graph-hub` suite (59 tests) and `weave-registry` binary tests pass identically with and without the flag. **Scope note**: this closes the "hub server accepts uploads without verifying" gap honestly — it does not, and cannot, provide non-repudiation (a shared secret key, not an asymmetric signature); that distinction is now stated plainly in the CLI flag's own help text rather than implied away.
- **✅ Verified (2026-09-13)**: confirmed — `grep -rn "verify_snapshot" crates/weave-graph-hub/src/*.rs` finds only the trait/impl definitions in `provenance.rs`; zero call sites in `server.rs` or `registry.rs`. `push_complete`/`commit_job` persist whatever `X-Weave-Signature` value arrives as an opaque `.sig` sidecar and never check it. This is distinct from — and not fixed by — this session's M3.6 work, which added the *client-side* `weave sync push --signature` seam (a caller can now attach a signature) but never touched server-side verification; this issue's finding stands exactly as written. Real, confirmed gap, high severity is justified. Terminology note: `provenance.rs`'s own `merkle_root()` function is a two-stage FNV-1a hash chain (`fnv1a(&bytes, 0)` then `fnv1a(payload, that)`), not an actual Merkle tree (no branching/leaf hierarchy, no partial-proof capability) — "Merkle" here is this codebase's own internal naming choice for a flat combined-hash signature, worth knowing before assuming Merkle-tree properties (e.g. proving one chunk without the whole payload) are available.
- **Location**: [`crates/weave-graph-hub/src/server.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-hub/src/server.rs) & [`registry.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-hub/src/registry.rs)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Row 42) defines Snapshot Provenance as verifying Merkle root cryptographic signatures over incoming `(repo, sha, payload)` archives. `SnapshotProvenanceVerifier` is implemented in `crates/weave-graph-hub/src/provenance.rs`, but `RegistryServer` request handlers and `Registry::push` never call `verify_snapshot` on `POST /snapshots/{repo_id}/{sha}.tar.zst` payloads before committing them to disk spools.
- **Mode & Security Impact**:
  - In **Self-Hosted Tier** (`--features custom`), snapshots pushed to centralized hub instances bypass Merkle root cryptographic verification, allowing tampered or forged graph databases to be committed to the registry.
- **Remediation**:
  - Wire `SnapshotProvenanceVerifier` into `handle_snapshot_push` before enqueuing write jobs into per-repo disk spools.
  - **Feasibility (2026-09-13) — mechanically easy, but doesn't add real security on its own**: calling `verify_snapshot` in `push_complete`/`commit_job` (`registry.rs`) before committing is a small, local change — the signature already arrives via `X-Weave-Signature` and is already persisted as a `.sig` sidecar, so the plumbing exists. The real blocker is key distribution, not wiring: the only shipped `SnapshotProvenanceVerifier` is `MockSnapshotProvenanceVerifier`, whose signing key is a well-known public constant baked into the OSS binary (this is *why* `weave sync push`'s own client side deliberately never signs with it by default — see `impl.md` M3.6). Verifying against that same public-key mock would be pure security theater: any pusher can forge a signature that "verifies," so the registry would look protected while being exactly as open as it is today. Real remediation needs a registry-side config knob for a deployment-supplied verifier/key (matching the "BYO signer" shape `weave sync push --signature` already establishes client-side), not just calling the existing mock's `verify_snapshot`.

---

#### HUB-01: Hub Canvas Endpoint Bypasses RBAC & Policy Filters
- **✅ Verified (2026-09-13)**: confirmed — `grep -n "rbac\|Rbac\|visible\|mask" crates/weave-graph-hub/src/canvas.rs` returns zero hits; `build_module_canvas` reads `storage.all_nodes()`/`all_edges()` directly with no visibility filter. Structural, not incidental: `weave-graph-hub` has no dependency on `weave_graph_core`'s `rbac` module at all, so there's currently no `RbacGuard` type reachable from this crate to apply — wiring this in is a real, larger cross-crate change, not a one-line fix. Real gap.
- **Location**: [`crates/weave-graph-hub/src/canvas.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-hub/src/canvas.rs)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Row 37 & Row 147) requires that architecture exports in Self-Hosted Tier emit LOD 1 diagrams with masked private subgraphs. In `canvas.rs`, `build_module_canvas` queries raw database nodes and edges directly without accepting an `RbacGuard` or applying LOD 1 module filters, serving unmasked internal topologies over HTTP.
- **Mode & Security Impact**:
  - In **Self-Hosted Tier**, external callers requesting diagrams from Hub receive unmasked internal module names, subpaths, and cross-boundary edges over HTTP.
- **Remediation**:
  - Migrate Hub diagrams to Level-of-Detail 1 (LOD 1) Mermaid representations, consuming pre-filtered `&[Module]` slices as proposed in `docs/proposal.md`.
  - **Feasibility (2026-09-13)**: the canvas is already LOD 1 module-level output (`canvas.rs::build_module_canvas`, added this session) — "Mermaid representations" would be a format change (JSON Canvas → Mermaid text), a real but separate ask from filtering. On the filtering half: full per-identity `RbacGuard` integration is arguably the wrong-sized fix here, since HUB-02 confirms this endpoint has no caller-identity concept at all (no auth means no "who is asking" to mask differently for) — wiring `RbacGuard` into an endpoint nobody authenticates to would mask by a fixed, hardcoded identity, not a real per-caller policy. A repo-level `[hub.canvas] exclude = [...]` path-glob (same shape SEC-02 proposes for `vector`) that hides specific modules from *everyone* is a more honest fit for an unauthenticated endpoint, and a smaller change. Sequence after HUB-02 if real per-caller masking is actually wanted.

---

#### HUB-02: Unauthenticated Hub Transport Exposure
- **✅ Fixed (2026-09-13)**: `weave-registry --auth-token` now gates every route on `Authorization: Bearer`; `HubClient`/`weave sync` attach it via `.weave/config.toml`'s `[hub] token`. See §9 Phase 1 row 1. (The "no auth middleware" verification note below predates this fix, from earlier the same day.)
- **✅ Verified (2026-09-13)**: confirmed, no auth middleware or token check anywhere in `server.rs`. Note the "token-authenticated HTTP" framing this cites from `feature_matrix.md` §2 Rows 41/43 is itself fictional — the real registry has never had any authentication (confirmed this session while auditing that file, now corrected there) — so there's no regression here, just a v1 design (`impl.md` M3.1: "a self-hosted hub on a trusted VPC/LAN") this issue correctly flags as worth hardening before any deployment binds beyond a fully trusted network.
- **Location**: [`crates/weave-graph-hub/src/server.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-hub/src/server.rs)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Rows 41 & 43) specifies token-authenticated HTTP communication for Centralized Hub operations. However, `weave-graph-hub` lacks an HTTP authentication middleware layer (`[hub.auth] token = "..."`), relying strictly on loopback socket binding (Core Invariant 6) for process isolation.
- **Mode & Security Impact**:
  - In **Self-Hosted Tier**, relies entirely on **Core Invariant 6** (binding to loopback `127.0.0.1` only). Any process with access to loopback can read raw repository graphs.
- **Remediation**:
  - Introduce `[hub.auth]` bearer token validation when binding beyond loopback or in multi-tenant environments.
  - **Feasibility (2026-09-13)**: real, and should probably be prioritized first among the Hub-layer fixes — HUB-01's canvas filtering and PROV-01's verification both become more meaningful once the registry can establish *some* caller trust. `server.rs`'s `read_request` already parses headers into a `Vec<(String, String)>` with a `header()` lookup helper (unlike the SCIM server's `read_request`, which doesn't — see IDP-02's note), so this is a smaller addition here than the equivalent SCIM fix: check one header against a configured token before dispatching in `handle_connection`. Low-medium effort, no blocker.

---

#### HUB-03: Lack of Centralized Mesh Policy Linting
- **✅ Verified (2026-09-13)**: confirmed — `server.rs`'s routing (`handle_connection`) only recognizes snapshot (`/snapshots/...`), canvas (`/repos/{id}/canvas`, `/mesh/canvas/...`, added this session), and webhook (`/repos/{id}/webhook`, added this session) paths; no policy-related route exists. Real, net-new capability gap — and note it would need FED-01's cross-repo policy-lint capability to exist client-side first (or a from-scratch server-side re-implementation), since today's `weave policy lint` has no federated/multi-repo mode to expose remotely.
- **Location**: [`crates/weave-graph-hub/src/server.rs`](file:///Users/ragu/Code/weave-graph/crates/weave-graph-hub/src/server.rs)
- **Root Cause**:
  `docs/feature_matrix.md` (§2, Row 35 & §3.4) specifies macro-mesh policy linting across multiple repositories ingested into the centralized Hub. However, `weave-graph-hub/src/server.rs` only defines routes for snapshot sync, webhooks, and canvas rendering, lacking a `/mesh/policy-lint` HTTP endpoint to evaluate global `policy.yaml` boundary rules across the aggregate multi-repo graph.
- **Mode & Governance Impact**:
  - In **Self-Hosted Tier**, enterprise platforms cannot run organization-wide macro-policy checks across all registered microservice snapshots in one unified call.
- **Remediation**:
  - Expose `GET /mesh/policy-lint` to evaluate global boundary rules against the aggregate multi-repo graph.
  - **Feasibility (2026-09-13)**: blocked on FED-01 — there is no federated/cross-repo policy-lint capability anywhere yet (client-side or otherwise) for this endpoint to expose. Building the endpoint before FED-01 exists would mean re-implementing federated boundary evaluation from scratch inside `weave-graph-hub` (which doesn't depend on `weave-graph-core::policy` today either), duplicating whatever FED-01 eventually builds client-side. Do FED-01 first; this becomes a thinner wrapper once it exists. Also inherits HUB-02's exposure concern one level up — a mesh-wide policy view across every registered repo is a bigger information-disclosure surface than the single-repo canvas HUB-01 already flags, so it shouldn't ship before HUB-02's auth either.

---
