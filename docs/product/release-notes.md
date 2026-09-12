# Release Notes

## Unreleased — Corrected Binary Sizes, Tree-Sitter Feature-Gating, Federated Query Persistence

Addendum to `v1.0.1` below, not a replacement — that entry's packaging-tier
sizes were pre-measurement estimates and are superseded by the real numbers
here (`cargo build --release`, this repo's actual `[profile.release]`:
`opt-level = "z"`, fat LTO, `codegen-units = 1`, stripped; measured
2026-09-12).

- **Real measured binary sizes (all tiers larger than originally estimated)**:
  - Standard Normal Mode, unflagged `cargo build --release` (today's actual
    default — links all 29 tree-sitter grammars): **41.1 MB** (43,112,948
    bytes), not the previously-stated <6.8 MB.
  - Standard Normal Mode, `--no-default-features` (8 core languages only,
    new this pass — see below): **9.6 MB** (10,083,852 bytes). This is the
    build that actually clears the <15 MB target; the unflagged default
    does not.
  - Vector Mode (`--features vector`): **41.2 MB** — a +88 KB delta over
    Standard, not +2.3 MB; `sqlite-vec` links statically and is small on
    its own, the grammar tables dominate either way.
  - Turso Mode (`--features turso`): **41.1 MB** — statistically identical
    to Standard; confirms `weave-graph-store-turso` is linked but not yet
    reachable from any CLI command.
  - Custom/Enterprise (`--features custom`): **41.6 MB**.
  - WASM (`crates/weave-graph-wasm`): **155.6 KB** (159,339 bytes) — this
    one was already accurate.
- **Tree-sitter grammars are now an opt-in Cargo feature** (`lang-extended`,
  on by `default` so today's full-language behavior is unchanged unless you
  build with `--no-default-features`): 21 of 29 grammars (everything beyond
  Rust/Python/JS/TS/Go/Java/C/C++) became `optional = true` dependencies.
  This is the fix the v1.0.0 "Known limitations" entry below said was
  "likely" and "not done in this release" — it's done now.
- **`weave link`'s composite graph is no longer thrown away**: it's
  persisted to `.weave/federation/<partner-label>.db` on both linked repos
  (same crash-safe stage-then-rename write path as `graph.db`), and a new
  `weave query-federated <repo-a> <repo-b> "<expr>"` command runs the same
  `callers`/`callees`/`path`/`impact`/`latency` query language `weave
  query` already supports, against that persisted cross-repo graph. No RBAC
  masking yet for federated queries — stated as an open gap, not silently
  skipped.
- **`weave report-federated <repo-a> <repo-b>`** (feature: `federation`):
  a unified `.canvas` architecture map across two linked repos, reusing
  `weave report`'s existing Louvain-clustered, 200-node-budgeted exporter
  against the persisted composite graph instead of a single repo — the
  root canvas now shows one node per linked repo instead of always
  exactly one. CLI-side only; no registry HTTP endpoint or webhook
  delivery (out of scope for this pass, tracked separately). No RBAC
  masking yet, same stated gap as `query-federated`.

## v1.0.1 — Packaging Tiers & Storage Engine Isolation

Previous release: `v1.0.0`.

### Changes & Packaging Tiers

- **Single-Engine Packaging**:
  - Standard Normal Mode (`v1.0.1`, executable `weave`): ultra-small, single-engine SQLite stripped binary (<6.8 MB macOS / <7.2 MB Linux), <80 MB peak RAM. Default GA/Stable distribution.
  - Vector Mode (`v1.0.1-vector`, executable `weave`): single-engine SQLite + `sqlite-vec` virtual tables with binary/int8 quantization (<9.1 MB stripped). Note: vector mode is exclusively supported on SQLite; Turso does not natively support vector virtual tables.
- **Turso Feature Mode (`v1.0.1-turso`, executable `weave`)**:
  - Available strictly for Normal Mode (no vector support), replacing SQLite with the libSQL embedded replica backend (<11.5 MB stripped).
- **CLI Self-Identification**:
  - `weave --version` now reports current binary version (`weave 1.0.1`).
- **Hub Ecosystem & Provenance Status**:
  - `hub-provenance` trait boundary (`SnapshotProvenanceVerifier`) implemented and tested standalone.
  - Not yet wired into `weave sync push/pull` — needs a registry-side storage schema change (signature sidecar) to actually transmit it. Stated honestly, not silently skipped.
  - `hub-canvas`, `hub-webhooks`, chunked upload: not started this pass.

## v1.0.0 — First Release

Tag `v1.0.0`, commit `7d2b8b9`.

The deterministic core (Phase 1) plus every scheduled Phase 2 feature
except the two documented exceptions below.

### Core (always available, no feature flag)

- Tree-sitter parsing across 29 languages into a symbol/call/structural-
  reference graph.
- Integer-compacted CSR adjacency (`petgraph::csr` + `roaring` bitmaps) for
  traversal queries.
- SQLite storage (`rusqlite`, bundled, WAL mode) with a migration runner
  that refuses to open a database newer than the binary understands.
- Crash-safe indexing: large rebuilds stage into `graph.db.rebuild` and
  atomically swap in — a crash mid-index never corrupts the active
  database.
- Bidirectional edge purge on incremental reindex — no dangling edges from
  a purged-and-reinserted file.
- `weave query` (`callers`/`callees`/`impact`/`path`), `weave report`
  (Markdown + `.canvas`), `weave export`.
- A local MCP server (`weave serve --mcp`) with 4 base tools, loopback-only
  by default, live reload on external reindex, and optional per-tool
  `max_tokens` response budgeting.
- `weave blast --base <ref>` — PR blast-radius comments, no GitHub
  networking from `weave` itself.

### Optional features shipped in this release

`docs`, `federation` (+ contract hashing / `weave check-contracts`),
`provenance`, `notes` (pinned agent/human notes), `watch` (auto-reindex
with blast-radius gating), `viz` (offline HTML report viewer), `hub`
(snapshot sync client), `slm` (natural-language querying for a human at a
terminal), `turso` (alternate libSQL storage backend), `python` (PyO3
bindings / `pip install` wheel). See [Features](features.md) for what each
one does and how to enable it.

### Known limitations

- **Platform support**: CI runs the full test suite on `ubuntu-latest` and
  `macos-latest` — Linux and macOS are the tested platforms for this
  release. There is no Windows entry in the CI matrix; `weave` is not
  verified to work there (some platform-conditional code exists, e.g. the
  `viz` feature's browser launcher, but it has never run in CI or been
  manually checked on Windows).
- **Binary size**: the default release build is ~43MB stripped, against a
  <15MB target. Root cause is the static grammar tables for 29 languages'
  Tree-sitter parsers (some grammars alone are 3–5MB), not a build
  misconfiguration — `[profile.release]` (LTO, `codegen-units = 1`,
  `strip = true`) is already correctly configured. Making individual
  languages opt-in Cargo features is the likely fix; not done in this
  release.
- **`hub`**: the client side (`weave sync pull|push`) is complete and
  tested. Two of the feature's three original acceptance criteria describe
  *hub-server* behavior (near-simultaneous-publish safety, retention
  pruning) — there is no bundled hub server in this repository to test
  those against, so they remain unverified until an actual deployment
  exists. This is expected, not a defect: `weave-graph-hub` is a client
  library, not a server.
- **`slm`**: real-model latency/accuracy numbers (time-to-first-token,
  end-to-end response time) require a downloaded GGUF model and
  `llama-cli`, neither available in the environment this release was
  built in. The deterministic router path (`FuzzyRouter`, used until you
  `weave slm pull` a model) is fully measured: ~1.8–1.9µs per prompt,
  roughly three orders of magnitude under the 5ms target.
- **`turso`**: implemented and tested as a library-level `Storage` backend
  with the same schema and trait as the default `rusqlite` backend, but
  not yet wired into the CLI's own storage selection — no `weave` command
  can choose it today. It measures ~15–25% slower than `rusqlite` on
  batch-insert throughput on this workload, which is why it isn't the
  default.
- **`weave link` / `[federation] linked_repos`**: `weave link <a> <b>`
  records contract expectations but does not yet append `linked_repos`
  back into `.weave/config.toml` for you — add the array entry by hand
  once (see [Configuration](configuration.md#federation)).

### Not in this release (planned for a later phase)

- `rbac` — query-layer access masking via an `AuthProvider` trait
  (Okta/Azure AD/SAML/OIDC).
- `otel` — OpenTelemetry/APM trace overlay on graph nodes.
- `policy-lint` — YAML architectural boundary rules with a CI gate.

None of the three above have any code or a Cargo feature flag yet; they're
scoped for a later, RBAC-first phase since every other Phase 3 feature
that touches visibility depends on that enforcement point landing first.

### Quality gates this release was held to

`cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D
warnings` clean on the default build; line coverage ≥90% workspace-wide
via `cargo-llvm-cov`; every optional feature verified to add zero new
dependency edges to a default build's `cargo tree`, with a full empirical
idle-RSS/latency diff against a captured core-only baseline for `slm` (the
one feature with a non-trivial resident-memory profile when active). See
the repository's internal `docs/impl.md` and `docs/performance_compare.md`
(not published — see the note at the top of this directory) for the full
milestone-by-milestone verification record.
