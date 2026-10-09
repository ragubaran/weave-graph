# Project Review and Viability Verdict

**Review date:** 2026-10-01  
**Verdict:** Pursue, but narrow the product sharply.

**Project stage:** `1.0.1` is the maturity target, with release scheduled for
**Saturday, 10 October 2026**. It is not yet a claim that a `v1.0.1` release
has been published. After that release boundary, the project moves into
PR-review mode: deterministic change-risk, contract, policy, and verification
checks integrated into pull-request workflows.

The engineering foundation is real and better than the project's lack of
traction suggests. However, Weave should not be pursued as another
general-purpose "code graph for AI agents." That space is already crowded by
Graft, Sourcegraph, GitNexus, Octocode, Codebase Memory, and similar tools.

The resulting product wedge is:

> A deterministic, air-gapped PR-risk and architecture-governance engine for
> multi-repository teams.

That means prioritizing blast radius, contract drift, policy enforcement,
auditability, and CI—not SLMs, notes, visualization, Python, WASM, Turso,
semantic search, and a hosted Hub simultaneously.

## Assessment

| Dimension | Score |
| --- | ---: |
| Engineering foundation | 8/10 |
| Current product readiness | 4/10 |
| Generic code-context differentiation | 3/10 |
| Differentiation as offline CI governance | 7/10 |
| Commercial and adoption evidence | 1/10 |
| Overall | Conditional go |

## What is genuinely solid

The review verified:

- 1,184 non-Python all-feature tests passed across 32 suites.
- `cargo fmt` is clean.
- Clippy is clean with all features and warnings denied.
- Workspace line coverage is 95.12%.
- The core dependency-closure check passes: no network, model, or inference
  runtime.
- The current core binary is 11,201,208 bytes, safely below 15 MiB.
- The mandatory mutually-referencing-file/dangling-edge regression passes.
- Full rebuilds use staged database promotion, and incremental reindexing
  purges and re-resolves affected edges.
- MCP loopback restrictions and Hub path/admission protections have meaningful
  tests.
- On the current 500k-symbol database, the long-lived MCP session measured
  10 MiB RSS and 2 ms average per query.

This is not vaporware. The storage, indexing, query, MCP, crash-safety, and
test foundations are substantial.

## Critical gaps

### 1. The headline 80 MiB invariant is currently false

The repository's own full-pipeline benchmark measured:

- Full `weave init && weave index`: **161 MiB peak RSS**
- Budget: **80 MiB**
- MCP session afterward: **10 MiB**

The project already records essentially the same failure in
[`issues.md`](issues.md#release-blocking-and-performance-gaps). The larger
problem is that CI's "comprehensive resource envelope" runs synthetic storage
examples, not the full CLI pipeline
([`verify_envelope.sh`](../scripts/verify_envelope.sh)). Therefore, CI can be
green while Core Invariant 4 fails.

The public GitHub description still advertises `<80MB RAM for 500k symbols`,
which should be removed until the full pipeline passes. The
[public repository](https://github.com/ragubaran/weave-graph) currently also
shows no meaningful adoption or release evidence.

### 2. Call-graph accuracy is the largest product risk

Resolution is principally a short-name index
([`resolve.rs`](../crates/weave-graph-parse/src/resolve.rs)). A method call such
as `self.step()` deliberately fans out to every method named `step`, even when
only one type is correct
([resolution tests](../crates/weave-graph-parse/src/resolve/tests.rs)). Some
generic languages extract symbols but miss calls entirely, as documented for
Haskell
([query-VM tests](../crates/weave-graph-parse/src/extract/query_vm/tests.rs)).

That is acceptable for exploratory navigation, but dangerous for:

- PR blocking
- Blast-radius claims
- Contract governance
- Architectural policy enforcement

There is no realistic edge precision/recall or blast-radius
false-positive/false-negative benchmark. Before selling governance, Weave
needs compiler/LSP/SCIP-grade resolution for a smaller set of priority
languages, or very explicit "heuristic only" behavior.

Competitors are already moving beyond syntax-only graphs. Graft offers
optional LSP-resolved edges and publishes agent benchmarks, while Codebase
Memory claims hybrid type resolution. Graft currently reports substantial
adoption and published SWE-bench/context benchmarks in its
[official repository](https://github.com/trailhq/Graft). Sourcegraph already
combines search, code graph, multi-repository context, and permission
enforcement in its
[context platform](https://sourcegraph.com/docs/cody/core-concepts/context).

### 3. The self-hosted security story is not production-ready

The Hub client explicitly rejects HTTPS
([`client.rs`](../crates/weave-graph-hub/src/client.rs)), while the deployment
guide tells users to configure an HTTPS registry URL
([`self-hosted.md`](product/self-hosted.md#60-registry-authentication--snapshot-provenance-hub-provenance)).
A conventional TLS-terminating reverse proxy is therefore unusable directly
by this client; users need plaintext transport or a separate local tunnel.

More seriously:

- `.weave/config.toml` is intentionally made committable
  ([`README.md`](../README.md#single-mode--one-repo-one-developer)).
- The same file is documented as containing Hub and SCIM bearer secrets
  ([`configuration.md`](product/configuration.md)).
- The client reads the Hub secret directly from that tracked file
  ([`sync.rs`](../crates/weave-graph-cli/src/sync.rs)).
- Registry authentication is passed through a CLI argument, exposing it to
  process listings.

Secrets need environment, secret-file, or keychain injection and local
untracked overrides. HTTPS support is required before presenting Hub sync as
an enterprise feature.

### 4. CI gives stronger confidence than it actually earns

Several declared gates are not real gates:

- Workspace coverage passes, but `weave-graph-core` measured **88.35% line
  coverage**. The per-crate script detects this failure, but CI does not invoke
  it ([`per_crate_coverage.sh`](../scripts/per_crate_coverage.sh),
  [`ci.yml`](../.github/workflows/ci.yml)).
- Benchmark comparison ends with `|| true`, so regressions cannot fail CI.
- A real 10% regression-gate script exists but is unused.
- The Python "coverage" step only prints two messages; it calculates no
  coverage.
- The full-pipeline memory test is not in CI.

These should be fixed before treating green CI as release evidence.

### 5. Scope and documentation have outrun the product

The repository contains many valuable but weakly integrated directions: Hub,
RBAC, provenance, SCIM, vector search, SLM routing, visualization, notes,
Python, WASM, Turso, OTEL, policy lint, federation, hooks, and PR review.

Several are honestly incomplete:

- Semantic search still uses a mock provider with 0.30 recall@10; no
  production learned-model quality evidence exists.
- SLM inference has not been validated with a real downloaded model.
- Turso is library-only and unreachable from the CLI.
- `weave link` still requires manual configuration editing.
- No published release or reliable install channel is currently evident.
- Internal status documents contradict newer code in several places.

There is also policy drift: release optimization is `opt-level = "z"` despite
the repository rules requiring `3` ([`Cargo.toml`](../Cargo.toml)), and
production code contains multiple `#[allow(...)]` suppressions despite the
rule requiring `#[expect]`.

## Market assessment

The need is real: coding agents benefit from persistent, structural context.
But "local Tree-sitter graph exposed over MCP" is no longer sufficient
differentiation.

Current alternatives already advertise:

- [Graft](https://github.com/trailhq/Graft): deterministic structural graph,
  MCP, freshness, call tracing, token budgets, LSP enrichment, and agent
  benchmarks.
- [Octocode](https://github.com/Muvon/octocode): Rust, semantic plus structural
  search, graph traversal, MCP, and a retrieval benchmark.
- [Codebase Memory](https://github.com/DeusData/codebase-memory-mcp): persistent
  graph, broad language support, impact analysis, and hybrid type resolution.
- [Sourcegraph](https://sourcegraph.com/docs/cody/core-concepts/context): mature
  multi-repository code intelligence and permission-aware retrieval.

Weave's opportunity is not beating all of those at context retrieval. Its
post-`1.0.1` PR-review direction is the stronger opportunity: combining
deterministic change-impact analysis with enforceable contracts and
architectural policy in an air-gapped CI artifact.

## `v1.0.1` maturity release plan

**Target:** publish `v1.0.1` on **Saturday, 10 October 2026** as the stable
maturity baseline. Feature expansion stops during the release window. After
publication, new product work moves to PR-review mode; `1.0.1` receives only
release-blocking fixes, security fixes, and packaging corrections.

The date is a target, not permission to waive a failed invariant. A failed
hard gate moves the release date; it must not be hidden by changing a test,
marking a required job advisory, or publishing an unsupported claim.

### Required release gates

All hard gates below must be evidenced against the exact commit that receives
the `v1.0.1` tag.

1. **Freeze and scope**
   - Freeze feature work and identify the release commit.
   - Confirm every workspace package and `Cargo.lock` resolve to version
     `1.0.1`, and `weave --version` prints `weave 1.0.1`.
   - Designate the supported artifact set. The current release workflow builds
     `weave` and `weave-custom`; do not publish `weave-custom` as stable unless
     its Hub, RBAC, provenance, and secret-handling paths pass the security
     gate.
   - Convert the `v1.0.1` release notes from draft only after all other gates
     pass.

2. **Code-quality and test gate**
   - `cargo fmt --all -- --check` passes.
   - Clippy passes across all supported targets and features with warnings
     denied.
   - Workspace, integration, CLI E2E, migration, and optional-feature tests
     pass from a clean checkout using the locked dependency set.
   - Workspace line coverage is at least 90%, and every supported crate also
     meets the repository's 90% per-crate requirement.
   - Python must have a real coverage result before being described as a
     supported release artifact; printing interpreter information is not a
     coverage gate.

3. **Correctness and data-safety gate**
   - The mutually-referencing-file incremental reindex regression passes with
     no edge endpoint referencing a missing node.
   - Full rebuild failure testing proves that the active database remains
     readable after interruption, and success-path testing proves that
     `graph.db.rebuild` is promoted atomically.
   - Schema migration tests pass from the oldest supported database through
     the current schema.
   - Call edges used for enforcement are either validated against the
     ground-truth benchmark or explicitly labelled heuristic. Heuristic edges
     must not silently become hard PR blockers.

4. **Resource-envelope gate**
   - The stripped core artifact remains below 15 MiB.
   - The full CLI indexing pipeline, not only the synthetic SQLite example,
     stays at or below 80 MiB peak RSS for 500,000 symbols on the retained
     Linux release runner.
   - Run the same full-pipeline measurement on macOS and retain the raw output
     as release evidence.
   - Default query latency and idle RSS remain unchanged when optional
     features are disabled.
   - The currently observed 161 MiB full-indexing result is a hard blocker
     until a later retained run demonstrates compliance.

5. **Security gate**
   - MCP still binds to loopback by default and requires an explicit opt-in for
     any broader bind address.
   - No supported workflow requires committing bearer tokens or passing
     secrets directly on a command line. Use environment, secret-file, or
     equivalent untracked injection.
   - Do not present Hub sync as production-ready over public networks until the
     client supports HTTPS or a precisely documented, tested local TLS tunnel
     is part of the supported deployment.
   - RBAC masking tests prove enforcement at storage/query boundaries for CLI,
     reports, exports, and MCP.

6. **Release-pipeline and packaging gate**
   - Make full-pipeline RSS, per-crate coverage, actual Python coverage, and
     benchmark regression blocking where their corresponding artifacts are
     supported.
   - Build the existing matrix for Linux x86_64 GNU, Linux x86_64 MUSL, Linux
     ARM64 MUSL, macOS Apple Silicon, macOS Intel, and Windows x86_64.
   - Produce SHA-256 checksums and verify every archive from the consolidated
     `SHA256SUMS.txt`; do not publish a platform whose build or smoke test is
     missing.
   - Install each retained artifact into a clean environment and run the
     documented `init`, `index`, `query`, and `report` smoke workflow.
   - Verify the Homebrew formula against the published asset before describing
     Homebrew as an available installation path.

7. **Documentation and claim audit**
   - Reconcile the feature table, CLI reference, install guide, release notes,
     README, and issue register with the exact release artifacts.
   - Remove or qualify any performance, platform, accuracy, or security claim
     without retained evidence from the release commit.
   - State that `pr-review` is Custom/self-hosted only and that its risk score
     is deterministic rather than LLM-generated.
   - Publish known limitations, including heuristic resolution boundaries and
     any feature intentionally withheld from `1.0.1`.

### Schedule

| Date | Milestone | Required outcome |
| --- | --- | --- |
| 1 October | Scope freeze | Freeze new features, nominate the release commit, assign every open hard gate, and choose the supported artifact set. |
| 2 October | Release/security scope | Decide whether `weave-custom` qualifies as stable; close unsafe secret transport or withhold the affected networked workflows. |
| 3–4 October | Correctness and memory | Close the full-pipeline 80 MiB blocker, rerun crash-safety, migration, incremental-edge, and call-resolution evidence. |
| 5 October | CI enforcement | Make the required resource, per-crate coverage, Python coverage, and benchmark checks genuinely blocking. |
| 6 October | Release candidate | Select the candidate commit, run the complete locked quality suite, and prohibit non-release changes after it passes. |
| 7 October | Platform matrix | Build all intended archives, verify checksums, and smoke-test each supported operating-system target. |
| 8 October | Distribution rehearsal | Test release-archive installation, validate Homebrew asset naming and formula changes, and reconcile all product documentation. |
| 9 October | Go/no-go review | Confirm every hard gate has retained evidence. Any unresolved hard gate produces a no-go and a revised date. |
| **10 October** | **Publish `v1.0.1`** | Create the immutable tag from the approved commit, run the release workflow, verify the GitHub release and checksums, smoke-test public downloads, then update the Homebrew tap. |
| 11–12 October | Release observation | Monitor installation and startup failures, publish corrections without moving the tag, and use a patch release for binary changes. |

### Release-day procedure

1. Confirm the approved commit is still the branch head and all required checks
   are green.
2. Create the annotated `v1.0.1` tag through the existing release workflow or
   push an equivalent reviewed tag; never reuse or move the tag after
   publication.
3. Let [the release workflow](../.github/workflows/release.yml) build the
   platform matrix, assemble the checksum manifest, and publish the GitHub
   release.
4. Download the public artifacts and `SHA256SUMS.txt` from GitHub rather than
   trusting only workflow-local files, then verify checksums and version output.
5. Run the [documented disposable-repository smoke test](product/install-1.0.1-testing.md#smoke-test)
   on the supported release archives.
6. Update and verify the Homebrew tap only after the GitHub assets are final.
7. Announce the release with the supported platforms, artifact variants,
   measured limits, and known limitations. If publication fails partway, do
   not move the tag; correct the workflow and issue a patch version if binaries
   changed.

### Post-release PR-review mode

From **11 October 2026**, PR review becomes the primary product track:

- Keep `1.0.1` as the stable maturity baseline and restrict it to critical
  fixes.
- Initially run `weave pr-review` as advisory output while collecting precision,
  recall, false-positive, and false-negative evidence.
- Permit hard blocking only for checks backed by deterministic evidence, such
  as confirmed contract drift or policy violations; keep heuristic call edges
  advisory until their accuracy gate passes.
- Use design-partner adoption and caught-defect evidence as the continuation
  test described below.
- Three local-only `pr-review` additions — P11.9 static HTML output, P11.10
  opt-in git-remote staleness check, P11.11 review-decision archive, scoped
  from a `backnotprop/plannotator` feature-gap review — were implemented and
  shipped 2026-10-08 on explicit instruction, ahead of the feature-freeze
  recommendation below (validation-sprint evidence has not been gathered for
  them any more than for the rest of Phase 11). See [`impl.md`](impl.md) §13
  for the full implementation record and test evidence.

## Recommended decision

After publishing `1.0.1`, fund a focused **6–8 week validation sprint**, not
another broad feature phase.

Priorities:

1. Freeze new features.
2. Fix or honestly relax the 80 MiB indexing claim.
3. Create a ground-truth call-edge and blast-radius benchmark for Rust,
   TypeScript, and Python.
4. Add HTTPS and safe secret injection.
5. Wire full-pipeline RSS, per-crate coverage, Python coverage, and benchmark
   regression into blocking CI.
6. Publish one supported binary with one reliable installation path.
7. Recruit five design partners who actually need offline or regulated PR
   governance.

Continue only if at least three teams use the PR/contract gate weekly and can
point to real defects or unsafe changes it caught. If users mainly want agent
context retrieval, stop competing head-on and either integrate with
Graft/SCIP/LSP or reposition Weave as the governance layer above them.
