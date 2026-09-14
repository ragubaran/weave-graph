# Audit Tests & Results Record

> **Document ID**: `audit_test.md`
> **Topic**: Verification record for the 2026-09-12 sessions — the `plan.md`↔`impl.md`↔code consolidation audit, the M2.0/M2.5/L8 remediation work, M3.4 (SCIM), M3.5 (migration planner), and the rayon parallel-parsing implementation (Ledger L10). Every number below is a real measurement on this machine (Intel i7-1068NG7, macOS x86_64, debug builds unless stated); ambient load conditions are stated where they matter.
> **Related**: [impl.md](impl.md) (milestone statuses and Phase 2 gap closures), [issues.md](issues.md) (remaining gaps), [plan.md](plan.md) (approved direction). Earlier `pending_p2.md` notes are incorporated in those records.

---

## 1. Rayon Parallel Parsing (Ledger L10) — Benchmarks

### 1.1 Corpus

Synthetic, generated for this audit: **4,000 Rust files / 16 MB / 48,000 symbols**, 20 modules × 200 files × 12 functions each, with realistic cross-file calls (each function calls two others, one in a neighboring module). Unresolvable references (`crate::modXX::worker::…`) deliberately included so the edge pass exercises its drop path. Ground truth after indexing: `Total Symbols: 48000`, `Total Edges: 96000` — asserted on every measured build.

### 1.2 Index wall-clock & peak RSS, per stage

| Stage | Full `weave index` wall | Peak RSS | Runs |
| :--- | :--- | :--- | :--- |
| **Baseline** (sequential, 3 parses/file: `build_project_index` + `upsert_all_nodes` + `upsert_all_edges`) | 43.94 s / 42.60 s | 45.1 / 46.5 MB | 2 |
| **Stage 1** (fuse passes 3→2: one parse feeds `ProjectIndex` + node upserts) | 22.40 s / 19.95 s | 48.1 / 46.3 MB | 2 |
| **Stage 2** (bounded chunked rayon, `PARSE_CHUNK = 32`) | 15.67 / 22.17 / 13.15 s | 48.4 / 49.1 / 49.5 MB | 3 |
| **Re-measured under heavy ambient load** (concurrent session compiling on the same machine) | 28.88 / 35.14 / 34.18 s | — | 3 |
| **Final, on the committed tree (`d8aa060`), machine quiet** | 14.65 / 16.90 / 15.91 s | 48.2 / 48.5 / 48.7 MB | 3 |

**Net: ~43 s → 14–17 s (−65%), RSS +~3 MB** (rayon pool + ≤32 in-flight `ParsedFile`s), against the 80 MB Core-Invariant-4 ceiling. Ground truth asserted on every measured build: `Total Symbols: 48000`, `Total Edges: 96000`. All timing pairs measured with `/usr/bin/time -p` (wall) and `/usr/bin/time -l` (max RSS), fresh `.weave` per run; the contended-load row is retained deliberately — even under compile storm, every run beats baseline.

### 1.3 Design invariants kept (each verified, not asserted)

- **Parse is the only parallel stage.** Chunks of 32 files parse across the rayon pool; the fold into `ProjectIndex`/SQLite upserts is strictly serial — writes stay funnel-shaped through the one SQLite connection inside the existing bulk-write transaction (L10's single-writer constraint).
- **RSS bounded by construction**: ≤32 `ParsedFile`s in flight (KBs each); measured peak confirms (+~3 MB over sequential).
- **`rayon` scoped to `weave-graph-cli`**: `weave-graph-core` remains rayon-free — the wasm32-unknown-unknown core build (M3.8's verified positive result) is untouched.
- **`parse_all` (federation) stays sequential** by choice: its `Vec<(PathBuf, ParsedFile)>` order is consumed directly by `repo_contract_hash`/federation report paths.
- **Determinism**: edge output is a deduped set (`distinct_edges: HashSet`); interning order changes do not change the stored graph — confirmed by identical symbol/edge counts (48000/96000) and the full test suite, including M1.4's blocking regression test (`reindex_one_of_two_mutually_referencing_files_leaves_zero_dangling_edges`).

---

### 1.4 Third measurement pass (2026-09-12) — recorded in `performance_compare.md` §5.4

New in this pass: the first real-repo corpus (Java) and the first end-to-end `weave index` measurement. All on the same hardware; **ambient-load caveat**: the concurrent session was compiling/testing during several runs, so load-skewed and quiet reference numbers are recorded side by side.

| Measurement | Third-pass result | vs. previous pass / target |
| :--- | :--- | :--- |
| Release binary size | **41.1 MB** | was ~43 MB; target <15 MB still missed (grammar tables — M3.8) |
| CSR memory | 12 B/node forward, 4 B/edge forward (24 B/node, 8 B/edge bidirectional) | resolved; ≤8 B/edge met (weight dropped, lazy reverse CSR — note 2) |
| CSR load (200k nodes) | 209 ms | — |
| Point lookup (`get_node`) | ~15–30 µs (contended; 7.2 µs quiet reference) | target <0.5 ms — ✅ >15× even contended |
| 3-hop traversal | ~44 µs (was ~25.8 µs — scheduler noise) | target <5 ms — ✅ >100× |
| Batch insert (rusqlite) | ~9.6k–21.5k nodes/s contended; **38.7k–43.4k/s quiet reference** | insert path byte-identical since the fix — spread is load |
| Batch insert (libSQL/turso) | ~17.2k/s (1k) / ~21.1k/s (10k) | at parity with same-run contended rusqlite (deferral rationale rests on the quiet-machine 25.7k/34.0k vs 31.8k/37.6k) |
| Core RAM @ 500k symbols | **39.5–41.4 MB** (release, 3 runs; full load 81 s) | target <80 MB — ✅ ~2× margin |
| **NEW: Java real corpus** | `google/guava` shallow clone: 3,275 files / 794,191 LOC / 29.3 MB; **2.4–2.5 MB/s** aggregate, 3,275/3,275 parsed, best of 3 passes (11.3–11.6 s/pass) via new `examples/corpus_throughput.rs` | §5.1's first Java corpus row; fixture-tiled figure stays ~4.0 MiB/s |
| **NEW: Full `weave index` end-to-end** | 4,000-file/16 MB/48k-symbol Rust corpus: **14.65–16.90 s**, 48.2–48.7 MB RSS (release, ground truth asserted) | first end-to-end measurement; L10 history 43 s → 20–22 s → 13–16 s |

Harness note: `criterion`'s per-case setup makes a 3,275-file corpus group take hours; `examples/corpus_throughput.rs` (committed, clippy/fmt clean) measures aggregate MB/s in one ~11 s pass instead.

## 2. Correctness Verification (after rayon + all session work)

| Check | Command | Result |
| :--- | :--- | :--- |
| Format | `cargo fmt --check` | clean |
| Clippy (default) | `cargo clippy --workspace --all-targets -- -D warnings` | 0 errors |
| Clippy (all features) | `cargo clippy --workspace --all-targets --all-features --exclude weave-graph-python -- -D warnings` | 0 errors |
| Tests (default) | `cargo test --workspace --exclude weave-graph-python` | 32/32 suites pass, 0 failures |
| Tests (all features) | `cargo test --workspace --exclude weave-graph-python --all-features` | 32/32 suites pass, 0 failures |
| Coverage gate (CI's exact command) | `cargo llvm-cov --workspace --exclude weave-graph-python --all-targets --fail-under-lines 90` | **91.14% lines** (94.16% line-column metric), exit 0 |

*Suite count grew 30 → 32 with the `weave-graph-wasm` crate (M3.8's WASM bindings, 7 tests) landing in the workspace in commit `d8aa060`. The three sync-test failures observed once under `--all-features` mid-session (`push_gives_up_after_exhausting_conflict_retries`, `push_on_main_publishes_the_snapshot`, `push_retries_past_a_second_conflict_before_giving_up`) were the concurrent session's in-flight edits and are fixed as of that commit — 0 failures on the committed tree, both feature sets. The session's aggregate also picked up their new tests: hub snapshot signatures (`push_with_signature_is_persisted_and_returned_on_pull`), `lock::acquire_timeout_*` (the ~5s lock timeout), `query::callers_propagates_storage_errors`, blast `exported_touched` tests, and CSR `from_nodes_and_edges` tests.*
| Blocking regression test | `reindex_one_of_two_mutually_referencing_files_leaves_zero_dangling_edges` (sqlite + turso) | pass |
| CLI index unit tests | `cargo test -p weave-graph-cli --bin weave index` | 8/8 pass (incl. the two new skip-branch/rebuild-cleanup tests) |
| CLI bin tests (features: federation, rbac, docs, notes, policy-lint, otel, slm, provenance, viz, watch) | `cargo test -p weave-graph-cli --features … --bin weave` | 193/193 pass |

**Note on the coverage aggregate** (92.17% → 91.21% → 91.14% across this session): the drift reflects *new code added this session* (SCIM server, migration planner, policy/trace modules, rayon chunking, the wasm bindings crate, plus the concurrent session's blast/sync/lock work) landing faster than its tests, not regressions in existing files. Every file modified by this session's work is ≥90% line coverage (the right-hand Lines column): docs 97.7%, rbac 94.8%, federation 98.4%, migration 96.9%, index 96.5%, markdown 95.9%, sqlite/turso backends ≥98%, hub client 92%, query 94.6%, traces 96.4%. The standing exception is `main.rs` (61.8%) — clap dispatch arms reachable only via the compiled binary, which subprocess-based e2e tests cannot instrument; pre-existing, repo-wide.

---

## 3. Feature-Isolation Gate (Ledger L8) — `scripts/feature_isolation.sh`

### 3.1 Final passing run (all 9 features)

| Build | Peak RSS (KB) | MCP latency (min-of-7 sessions, 200 queries each) |
| :--- | :--- | :--- |
*(mid-session run, contended machine)*: default 3,048 KB / 260 ms; docs 3,092 / 210; federation 3,100 / 270; provenance 3,084 / 260; notes 3,076 / 240; watch 4,132 / 240; viz 3,108 / 230; rbac 3,088 / 230; fts 3,132 / 270 — PASS.

*(final run on the committed tree, machine quiet)*:

| Build | Peak RSS (KB) | MCP latency (min-of-7 sessions, 200 queries each) |
| :--- | :--- | :--- |
| default | 3,080 | 150 ms |
| docs | 3,144 | 150 ms |
| federation | 3,144 | 170 ms |
| provenance | 3,136 | 160 ms |
| notes | 3,100 | 150 ms |
| watch | 4,240 | 160 ms |
| viz | 3,164 | 150 ms |
| rbac | 3,196 | 140 ms |
| fts | 3,128 | 140 ms |

**PASS** — max RSS delta +1.2 MB (watch, the `notify` dependency) against the 8 MB tolerance; latency spread 140–170 ms, no outlier near the threshold.

### 3.2 How the latency measurement was hardened (each failure was a real finding)

1. **Spawn-per-query loop (v1)** measured watch at +17–23% "regression". Investigation: in-process latency (one `serve --mcp` session, 20 queries) was *identical* between default (~2.85 s) and watch (~2.9 s). The 17% was **binary page-in cost at spawn** — `notify` legitimately grows the binary — not query-path latency. The gate was measuring the wrong thing; rewritten to one MCP session per timing sample.
2. **Silent script death (v2)**: the MCP session ran from the repo root, not the fixture — the server died instantly ("no graph database found") and `set -o pipefail` turned its non-zero exit into a silent `set -e` kill. Fixed (`cd '$WORK' &&` inside the timed command).
3. **Session-level jitter (v3)**: default and feature builds measured minutes apart read as 10–45% fake deltas on a ~300 ms session. Fixed two ways: all binaries are built **up front**, then measured back-to-back; and per-binary latency is **min-of-7** sessions (load noise is one-sided — it only adds time — so min approaches the true unloaded cost). Threshold set to 30% with the rationale in-script: the signal being gated is gross (an eager model load or thread pool shows up as seconds, not percent).
4. Verified stable: watch 3/3 passes after the fix; full 9-feature run passes; federation+watch run passes.

### 3.3 What the gate deliberately does NOT claim

Per-binary spawn wall-clock is *not* gated — binary-size-driven page-in is a packaging property (M3.8's territory), not an idle-cost property. The L8 claim this script asserts is the `AGENTS.md` §1.8 one: **idle RSS delta** and **in-process query latency**.

---

## 4. Consolidated Documentation Audit (plan.md ↔ impl.md ↔ code)

Verified feature-by-feature; drift corrected in the docs themselves. Summary of the checks (detail in `impl.md` §5's sequencing summary and `plan.md` §0.3's implementation-status note):

- **All 16 CLI feature flags** match shipped, tested code (`notes docs federation provenance hub slm rbac otel policy-lint turso python watch viz fts vector hub-provenance`).
- **plan.md drift fixed**: `viz` "Deferred — no code yet" → Done (M2.13); `custom` bundle sentence described the pre-M3.0 state → updated to the wired reality (`team+hub+provenance+rbac+otel+policy-lint+fts+vector`); crate table missing `weave-graph-python` → added; §0.3 config keys split into "read by the binary as written" vs "aspirational" (`storage.backend`, `relocate_on_network_fs`, `index.bailout_*` — `cmd_index` passes `ReindexConfig::default()`; `auth.provider`; `slm.*`); §1.2a rayon and §1.4 lock-timeout notes annotated with implementation status.
- **impl.md sequencing summary consolidated**: M2.5 ✅ (server criteria closed by M3.1), M2.0 gaps 1–3 closed, L8 ✅, M3.0–M3.5 ✅, M3.6/M3.7 🚧 partial (exact remainders listed), M3.8 ❌ with per-sub-effort blockers, Phase 3 Exit Criteria explicitly open, Phase 4 not started.
- **Known issues — flagged here first, since closed (2026-09-12, concurrent session)**: the hub retention test's parallel-load flake (fixed with tie-breaker sorting + polling against `pending_jobs`), `query.rs::callers_text`'s error-swallowing `unwrap_or_default()` (now propagates), and the advisory lock's missing ~5s timeout (now `acquire_timeout`) were all flagged during this audit and are fixed. **Still open, by design**: AST parser throughput (~4 MiB/s vs >25 MB/s target) and release binary 41.1 MB vs <15 MB (26 tree-sitter grammar tables — M3.8's sized-not-started fix). (CSR memory target <=8 B/edge is resolved and Met: 4.0 B/edge forward, 8.0 B/edge bidirectional).

---

## 5. Session Test Inventory (new tests this work added)

| Area | Test | Asserts |
| :--- | :--- | :--- |
| M2.0 §anchors | `section_anchor_resolves_to_doc_section_or_falls_back` | `[[Note#Section]]` → `doc_section` node when the heading exists; parent-note fallback when it doesn't; no fabricated sections |
| M2.0 §topics | `notes_link_tagged_to_their_topics` | every note owns `TAGGED` edges to its topics |
| M2.0 §GC | `removing_a_tag_gcs_the_orphaned_topic_node` | incremental tag removal sweeps the orphaned `doc_topic` |
| M2.0 §refs | `backtick_refs_prefer_the_same_directory_candidate` | ambiguous short-name code refs narrow to the single same-dir candidate; fan-out otherwise |
| M2.0 parser | `headings_are_extracted_in_document_order` | `ParsedMarkdown.headings` in order, ATX closing sequences trimmed, heading text never leaks into the wikilink scanner |
| Index robustness | `unparseable_unsupported_and_unreadable_files_are_skipped_not_fatal` | `Some(Err)`/`None`/read-failure/dangling-call branches skip without aborting; 0 edges from unresolvable calls |
| Index rebuild | `stale_rebuild_files_are_removed_before_both_reindex_paths` | leftover `.rebuild` files are replaced by both reindex paths |
| M3.4 SCIM | `provisioned_then_deprovisioned_user_loses_query_access_on_the_next_sync_cycle` | M3.4's verify: stale snapshot denies → sync grants → deprovision still grants → sync denies; merged guard does not resurrect |
| M3.4 SCIM | `scim_provision_requires_a_username_and_deprovision_404s_unknown_users` | 400 on empty `userName`; 404 on unknown delete; failed mutations create no file |
| M3.4 SCIM | `scim_listing_and_get_reflect_the_snapshot_not_the_file` | `GET /Users` snapshot semantics; default `reader` role; 404 unknown |
| M3.4 SCIM | `scim_server_binds_loopback_and_serves_real_tcp` | loopback bind + real-TCP provision/sync/list round trip |
| M3.4 SCIM | `scim_server_binds_loopback_and_serves_real_tcp` / `cmd_serve_scim_runs_a_real_server_on_a_real_port` | the CLI command serves real TCP and writes `.weave/rbac-directory.toml` |
| M3.4 SCIM | `scim_rejects_an_empty_username_and_unsupported_methods` / `directory_parsing_is_tolerant_of_malformed_content` | 405 unsupported method; tolerant TOML parsing; empty-username rejection |
| M3.4 guard | `guard_for_prefers_scim_directory_roles_over_config` | IdP-managed directory overrides `[rbac.users]` for the same subject |
| M3.5 planner | `a_linear_chain_plans_callers_before_the_provider` | M3.5's verify: 3-repo linear chain orders both callers before the provider; caller files named; provider step states the removal action |
| M3.5 planner | `a_cross_repo_cycle_is_reported_not_arbitrarily_ordered` | cycle → error naming the cycle, no plan file written |
| M3.5 planner | `an_unknown_symbol_and_an_uncalled_symbol_are_clear_errors` | clear errors for unknown symbol / no cross-repo callers |
| M3.2 policy | `cmd_lint_blocks_violation_and_passes_compliance`, `cmd_lint_requires_an_existing_index`, `cmd_drift_reports_without_blocking`, `cmd_lint_skips_rbac_masked_edges_and_says_so`, `cmd_drift_reports_a_cycle_and_no_orphans`, `cmd_lint_surfaces_confirmed_adr_obligations_advisory` | CI gate exit codes, config-error rejections, RBAC-masked skip counting, drift branches, slm advisory composition |
| L8 gate | `scripts/feature_isolation.sh` runs in CI as its own `feature-isolation` job | per-feature RSS delta + in-process MCP latency within tolerance |

All of the above pass at the time of writing, under both the default and `--all-features` builds, with `cargo fmt --check` and `-D warnings` clippy clean in both configurations.

---

## 6. Final consolidated state (commit `f0b210d` (d8aa060 amended with the corpus-throughput harness), 2026-09-12)

| Gate | Result |
| :--- | :--- |
| `cargo fmt --check` | clean |
| clippy, default + all-features, `-D warnings` | 0 errors |
| Tests, default + all-features | 32/32 suites each, 0 failures |
| Coverage gate (`--fail-under-lines 90`) | 91.14% lines, exit 0 |
| L8 feature-isolation gate | PASS (9 features, max ΔRSS +1.2 MB) |
| Rayon parse (L10) | 43 s → 14–17 s on 4k files, RSS 48–49 MB |
| Blocking regression test | pass (sqlite + turso) |

Work landed this session: M2.0 gap closures (section anchors, topic GC, same-dir backtick refs), M2.5 packaging + stances (`deploy/`), L8 CI gate, M3.2 `policy-lint`, M3.3 `otel`, M3.4 SCIM directory sync, M3.5 migration planner, L10 rayon (fused passes + bounded chunks), and the plan↔impl documentation consolidation. The concurrent session landed in the same commit: `weave-graph-wasm` bindings, hub snapshot signatures (M3.6's transport wiring), vector/FTS search surfaces, lock timeout, blast `exported_touched`, and the robustness fixes flagged in §4.
