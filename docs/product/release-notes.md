# Release Notes

## Unreleased — RBAC Hardening, Registry Auth & Provenance, Storage Trait Cleanup, New MCP Tools, Corrected Binary Sizes, Federated Query Persistence, Zero-Config MCP & Ignore Management

### Zero-Config AI Agent MCP Integration & Smart Ignore Management (`weave init`)

Developer experience improvements to streamline onboarding with AI coding assistants and repository ignore configurations:

- **Auto-Registration of MCP Server (`.mcp.json`)**:
  - `weave init` automatically provisions `.mcp.json` at the project root with the `weave` server configuration (`{"command": "weave", "args": ["serve", "--mcp"]}`).
  - **Non-Destructive Merging**: If `.mcp.json` already exists (e.g. configuring `graft`, `filesystem`, or custom MCP tools), `weave init` parses the existing JSON and safely merges `weave` into `mcpServers` without altering any existing servers or options.
  - **Universal AI Agent Support**: Compatible out of the box with Claude Code, Cursor, Windsurf, Google Antigravity, Gemini Code Assist, GitHub Copilot, Codex, OpenCode, Hermes Agent, and Kiro.
- **Smart Ignore File Resolution (`.gitignore` & `.ignore`)**:
  - Automatically configures `.gitignore` with `.weave/*` and `!.weave/config.toml` so all derived SQLite graphs (`.weave/graph.db`), advisory lock files, and rebuild staging directories (`.weave/graph.db.rebuild`) are ignored while `.weave/config.toml` remains trackable and committable in version control.
  - Automatically converts existing blanket `.weave/` or `.weave` entries to `.weave/*` and `!.weave/config.toml`, avoiding Git's behavior where directory exclusions suppress contained unignore rules.
  - If `.ignore` is present (used by `ripgrep` / `ag` to configure search visibility), configures `!.weave/`, `.weave/*`, and `!.weave/config.toml` so search tools re-admit `.weave/config.toml` without scanning binary databases.
  - If neither exists, creates `.gitignore` with `.weave/*` and `!.weave/config.toml` by default.
  - Idempotent: checks before writing to prevent duplicate entries across repeated `weave init` runs.
- **AST Indexer Dotfile Filtering**:
  - Updated `is_indexable` in `weave-graph-cli` to ignore hidden dotfiles (files whose names start with `.`). Configuration files such as `.mcp.json`, `.gitignore`, and `.ignore` will no longer be treated as source code or indexed into the symbol table.
- **Full E2E & Unit Test Coverage**:
  - Verified with unit tests in `crates/weave-graph-cli/src/tests.rs` (testing file creation, JSON merging, ignore file precedence) and binary E2E tests in `crates/weave-graph-cli/tests/cli_e2e.rs` using `assert_cmd` and `predicates`.

### RBAC, Registry Auth & Provenance, Storage Trait, New MCP Tools

Security and API-surface fixes from the Phase 3 issue audit, applied across `rbac`, `hub`, `hub-provenance`, `mcp`, and the `Storage` trait:

- **`weave serve --mcp --require-as`** (feature `rbac`, new flag) and **`[rbac] require_identity = true`** (`.weave/config.toml`, new key): either refuses to start the MCP server at all if `--as <subject>` is omitted. Closes an operational footgun for shared/multi-tenant deployments (a proxy or CI runner that forgot `--as` previously got a fully unmasked session); every other RBAC-gated command's own "no `--as` == unmasked" default is unchanged.
- **`weave check-contracts`/`weave blast` waivers**: an identity-less waiver (`--as` omitted) is now rejected outright if this repo's own `[rbac.users]` config grants the `"allow-drift"` role to _anyone_ — closing a bypass where a contractor blocked by `--as carol` could simply drop `--as` and waive unrestricted. A repo that never grants `"allow-drift"` to anyone sees no behavior change.
- **`weave rbac serve-scim` bearer-token auth** (new): optional `[rbac.scim] token` in `.weave/config.toml` requires a matching `Authorization: Bearer` header on every request; unset keeps the previous unauthenticated loopback-trust behavior.
- **`weave-registry --auth-token`** (new): requires `Authorization: Bearer <token>` on every registry request when set; client side reads `[hub] token`. Unset keeps the previous unauthenticated behavior.
- **`weave-registry --provenance-key`** (new, feature `hub-provenance`): verifies a push's `X-Weave-Signature` (hex-encoded bytes) against a deployment-supplied secret (`MockSnapshotProvenanceVerifier::with_key`, never its public default key) _before_ the push is committed — a bad, missing, or tampered signature is rejected (`400`) and never advances the repo's head or consumes a rate-limit slot. Unset keeps every push unverified, as before.
- **SCIM role parsing accepts RFC 7643 object arrays** (`[{"value": "internal", "primary": true}]`), not just the previous flat-string-array shape — real Okta/Azure AD/Google Workspace payloads previously resolved to silently-empty roles.
- **`weave policy drift` orphan reports** now annotate an orphan with `(has hidden inbound edges)` when it's only an orphan because RBAC masking severed its real inbound edge — distinguishing a genuine orphan from a masking artifact.
- **Semantic search (`weave search --semantic`, `weave_search_semantic`)**: the RBAC visibility filter is now applied to reranked candidates _before_ the result is truncated to `limit`, not after — a masked top hit can no longer starve a visible runner-up out of a size-capped result (previously: the top-K could be entirely masked, returning zero results even when visible matches existed further down).
- **`search_symbols`/`search_vector` moved onto the `Storage` trait** (`weave-graph-core`), with a default "unsupported" implementation — any current or future backend gets both without stub work; previously these were inherent methods only `SqliteStorage` had.
- **Two new MCP tools**: `weave_search_semantic` (feature `vector`) and `weave_policy_lint` (feature `policy-lint`), both masked through the same session-bound `RbacGuard` every other tool already uses. `weave serve --mcp` now advertises up to 8 tools (4 base + 2 `notes` + 1 `vector` + 1 `policy-lint`), up from 4–6.
- **Documentation correction**: there is one CLI binary (`weave`), and
  `--features turso` only compiles the tested `weave-graph-store-turso`
  library alongside the default SQLite backend. The CLI never constructs
  `TursoStorage` and provides no backend selector or Turso distribution. A
  future CLI integration requires its own build, compatibility, and recovery
  decision; it is not a current product capability.

### Corrected Binary Sizes, Tree-Sitter Feature-Gating, Federated Query Persistence

Addendum to `v1.0.1` below, not a replacement — that entry's packaging-tier
sizes were pre-measurement estimates and are superseded by the real numbers
here (`cargo build --release`, this repo's actual `[profile.release]`:
`opt-level = "z"`, fat LTO, `codegen-units = 1`, stripped; measured
2026-09-14).

- **Real measured binary sizes (all tiers larger than originally estimated)**:
  - Standard Normal Mode, unflagged `cargo build --release` (extended-language
    build): measured size is retained as historical evidence only; it is not
    the core-size target.
  - Standard Normal Mode, `--no-default-features`: **10,128,832 bytes
    (9.66 MiB)**, below the 15 MiB core target.
  - Vector mode: **10,241,872 bytes (9.77 MiB)** in the current mock-provider
    build; learned-model quality and whole-pipeline memory remain unverified.
  - Turso mode package size remains unverified and the backend is not reachable
    from the current CLI command path.
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

## v1.0.1 — Packaging Tiers & Storage Engine Isolation (draft release notes)

Previous release: `v1.0.0`.

### Changes & Packaging Tiers

- **Single-Engine Packaging**:
  - Standard Normal Mode: the core artifact target is the stripped `--no-default-features` build. Do not use this entry as evidence of a published release or certified RAM envelope.
  - Vector mode is optional storage groundwork; package size, learned quality, and ANN performance are not certified.
- **Turso library feature**: `weave-graph-store-turso` is implemented and
  tested as a library backend, but no `v1.0.1-turso` executable or CLI backend
  selector is published.
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

- Tree-sitter parsing across the languages compiled into the selected build into a symbol/call/structural-
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
  <15MB target. Root cause is the static grammar tables for extended languages'
  Tree-sitter parsers (some grammars alone are 3–5MB), not a build
  misconfiguration — `[profile.release]` (LTO, `codegen-units = 1`,
  `strip = true`) is already correctly configured. Making individual
  languages opt-in Cargo features is the likely fix; not done in this
  release.
- **`hub`**: the client side (`weave sync pull|push`) is complete and
  tested. Two of the feature's three original acceptance criteria describe
  _hub-server_ behavior (near-simultaneous-publish safety, retention
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

- Built-in SSO/OIDC/SAML login and vendor-specific IdP adapters. The current
  `rbac` release provides query-layer masking plus generic SCIM provisioning;
  an external IdP must push subjects and roles to the loopback SCIM endpoint.
- `otel` — OpenTelemetry/APM trace overlay on graph nodes.
- `policy-lint` — YAML architectural boundary rules with a CI gate.

The SSO/OIDC item above has no code or Cargo feature flag. RBAC, SCIM,
OpenTelemetry, and policy-lint are available only in their documented
feature builds; do not infer interactive SSO from SCIM support.

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
