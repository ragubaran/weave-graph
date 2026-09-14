# Unapproved proposals

This file is the home for designs awaiting explicit approval. Accepted Phase 4
profile and sequencing decisions live in [plan.md](plan.md), with development
tasks in [impl.md](impl.md). Open problems, including the hub diagram
authorization question, live in [issues.md](issues.md). The design below is
a draft, not a statement that these output formats, flags, or endpoints ship.

The former `SLLaM.md` sketch also proposed an offline, maintainer-run
fine-tuning experiment using graph-checked question/path pairs. It is **not**
approved product work, not part of the `slm` Cargo feature, and carries no
validated model-speed or accuracy figures. If revisited, it needs a real
evaluation set, licensing/data review, hardware budget, and a demonstration
that it improves explicit codebase Q&A or feature-design requests beyond the
deterministic retrieval baseline. No teacher-model pipeline belongs in the
normal developer build.

## Mermaid architecture-map format (draft)

**Status**: Draft
**Scope**: `weave-graph-hub` (`hub-canvas` feature), `weave-graph-cli` (`report`, `viz`, `federation`)

---

## 1. Problem Statement

The current architecture map pipeline produces [JSON Canvas](https://jsoncanvas.org) (`.canvas`) files — a format tightly coupled to Obsidian:

- **Vendor lock-in**: `.canvas` is only natively renderable in Obsidian. GitHub, GitLab, and most documentation platforms cannot display it.
- **CI/CD friction**: Pipeline artifacts (`.canvas`) require Obsidian or the custom `weave viz` HTML viewer to be useful. Reviewers cannot preview architecture maps in PR diffs.
- **Duplicate rendering**: The `viz.rs` module exists solely to convert `.canvas` → standalone HTML because no platform renders `.canvas` natively.
- **Two implementations**: `hub/canvas.rs` and `cli/report.rs` both independently build `CanvasNode`/`Canvas` JSON structures from the same Louvain modules — same algorithm, two serialization targets.

Mermaid is already used in 4 docs files (`pipeline.md`, `mcp-integration.md`) and renders natively on GitHub, GitLab, Obsidian, VS Code, and most documentation platforms.

---

## 2. Design Constraints

### 2.1 RBAC & Policy Enforcement (Custom Mode)

> [!CAUTION]
> In custom mode (`--features custom`), **all hub-related features must enforce RBAC and policy at the query/storage layer** — Core Invariant 7. No exceptions for diagram output.

**Current gap (pre-existing)**: `hub/canvas.rs` `build_module_canvas()` calls `storage.all_nodes()` and `storage.all_edges()` with zero RBAC filtering. The CLI's `report.rs` correctly accepts a `visible` filter — the hub does not.

**Rule for this proposal**:
- `render_mermaid()` must consume **pre-filtered** `&[Module]` and `&[Edge]` — data already passed through `RbacGuard::visible()` by the caller.
- The renderer never calls `Storage` directly. RBAC lives at the query boundary, not inside a formatting function.
- Hub's `build_module_canvas()` must wire `RbacGuard` filtering when `rbac` feature is active. Hidden modules are dropped entirely, not rendered with masked labels.

### 2.2 Diagram Detail Level — High-Level Linking Only

> [!IMPORTANT]
> Hub-served diagrams show **module-level architecture only** — module labels and inter-module edges. No file paths, no symbol names, no line numbers.

This is both a privacy boundary and a readability choice:

| Detail | Hub Diagram | CLI Diagram |
|:---|:---:|:---:|
| Module labels (e.g. `auth`, `storage`) | ✅ | ✅ |
| Module file counts (e.g. `12 files`) | ✅ | ✅ |
| Inter-module edge weights | ✅ | ✅ |
| File paths inside modules | ❌ | ✅ (LOD 2) |
| Symbol names / signatures | ❌ | ✅ (LOD 3 via `export`) |
| Line numbers | ❌ | ✅ (LOD 3 via `export`) |

Hub diagrams are LOD 1 only. LOD 2 (per-module file detail) and LOD 3 (symbol-level) remain CLI-only, generated locally where RBAC `visible` filtering is already wired and file-level detail is appropriate.

### 2.3 Policy-Lint Integration

When `policy-lint` feature is active alongside `hub-canvas`:
- Policy violations (boundary crossings, cycles) may be annotated on the high-level diagram as edge styling (e.g. `-->|"⚠ boundary"|` in Mermaid)
- Policy annotations receive the same pre-RBAC-filtered input — hidden nodes/edges are dropped upstream, never passed to `policy::lint()`
- Policy-lint is not a hub dependency today; if added, it must follow the same pre-filtered-data rule

---

## 3. Proposed Change

Replace `.canvas` JSON output with Mermaid diagram text (`.mmd` files) across all three touch points:

| Layer | Current | Proposed |
|:---|:---|:---|
| `weave report` (CLI) | `weave-report.canvas`, `weave-modules.canvas`, `weave-module-{id}.canvas` | `weave-report.mmd`, `weave-modules.mmd`, `weave-module-{id}.mmd` |
| `weave viz` (CLI) | Reads `.canvas` → renders HTML via embedded viewer template | Reads `.mmd` → renders HTML with Mermaid.js CDN or embedded runtime |
| `hub-canvas` (Hub) | `GET /repos/{id}/canvas` returns JSON Canvas | `GET /repos/{id}/canvas` with `Accept` header negotiation: `text/plain` → Mermaid (default), `application/json` → legacy JSON Canvas. LOD 1 only |
| `weave report-federated` | Produces merged `.canvas` | Produces merged `.mmd` |
| Configuration | `[report] format = "canvas"` | `[report] format = "mermaid"` (default), `"canvas"` (legacy) |

---

## 4. Blast Radius — Files Requiring Changes

### 4.1 Core Rendering (New Shared Module)

```text
crates/weave-graph-core/src/render_mermaid.rs        [NEW]
```

- Single shared Mermaid text emitter consuming **pre-filtered** `&[Module]` and inter-module edges
- LOD 0 (repo overview): `graph TD` with one node per repo_id
- LOD 1 (modules): `graph TD` with module nodes + inter-module edges (hub + CLI)
- LOD 2 (per-module files): `graph TD` with file nodes inside module (CLI only)
- Pure function `fn render_mermaid(modules, edges, lod) -> String`
- No I/O, no dependencies, no RBAC logic — stays inside `weave-graph-core`'s zero-network boundary
- RBAC filtering is the caller's responsibility, not this module's

### 4.2 CLI Report Module

```text
crates/weave-graph-cli/src/report.rs                 [MODIFY]
```

- Replace `CanvasNode`/`Canvas`/`CanvasEdge` structs with calls to `render_mermaid()`
- Output `.mmd` files instead of `.canvas`
- `ReportPaths.canvas_files` → `ReportPaths.diagram_files`
- Preserve overflow budget logic (NODE_BUDGET = 200)
- Existing `visible` RBAC filter already filters before module building — no change needed

### 4.3 CLI Viz Module

```text
crates/weave-graph-cli/src/viz.rs                    [MODIFY]
crates/weave-graph-cli/src/viz/canvas_viewer.html    [MODIFY]
```

- `render_html()` embeds `mermaid.min.js` via `include_str!` — offline-first, no CDN
- Only embedded when `[report] format = "mermaid"` or `"all"` — canvas-only reports skip it
- Inline a `<pre class="mermaid">` block with the embedded runtime
- File detection: `.mmd` instead of `.canvas`
- `content_type()`: `"text/plain"` for `.mmd`

### 4.4 Hub Canvas Module

```text
crates/weave-graph-hub/src/canvas.rs                 [MODIFY]
crates/weave-graph-hub/src/server.rs                 [MODIFY]
```

- `build_module_canvas()` → returns `String` (Mermaid text) instead of `Canvas` struct
- `CanvasNode`/`Canvas` structs removed (replaced by core's `render_mermaid`)
- `grid_layout()` removed (Mermaid handles layout)
- `handle_canvas()` response `Content-Type` → `text/plain`
- `from_snapshot_bytes()` return type: `Result<String, String>`
- **New**: When `rbac` feature is active, `build_module_canvas()` must accept and apply `RbacGuard::visible()` filter before building modules
- Output is strictly LOD 1 — module labels + counts + inter-module edges only

### 4.5 Hub Cargo Feature Dependencies

```text
crates/weave-graph-hub/Cargo.toml                    [MODIFY]
```

- `hub-canvas` feature no longer needs `dep:serde`, `dep:serde_json` (Mermaid output is plain text)
- Reduced to: `hub-canvas = ["dep:weave-graph-store-sqlite"]`
- Optional RBAC integration: `hub-canvas-rbac = ["hub-canvas", "weave-graph-core/rbac"]`

### 4.6 Federation Module

```text
crates/weave-graph-cli/src/federation.rs             [MODIFY]
```

- `canvas_files` references → `diagram_files`
- `weave report-federated` emits `.mmd` instead of `.canvas`

### 4.7 Configuration & Documentation

```text
docs/product/configuration.md                        [MODIFY]
docs/product/cli-reference.md                        [MODIFY]
docs/product/features.md                             [MODIFY]
docs/product/getting-started.md                      [MODIFY]
docs/product/pipeline.md                             [MODIFY]
docs/product/release-notes.md                        [MODIFY]
README.md                                            [MODIFY]
```

- All `.canvas` references → `.mmd`
- `[report] format` default value change documented
- Document hub diagram detail restriction (LOD 1 only)

### 4.8 Tests

```text
crates/weave-graph-core/src/render_mermaid/tests.rs  [NEW]
crates/weave-graph-cli/src/report/tests.rs           [MODIFY]
crates/weave-graph-cli/src/viz/tests.rs              [MODIFY]
crates/weave-graph-cli/tests/cli_e2e.rs              [MODIFY]
crates/weave-graph-hub/src/canvas/tests.rs           [MODIFY]
crates/weave-graph-hub/src/registry/tests.rs         [MODIFY]
crates/weave-graph-hub/src/server/tests.rs           [MODIFY]
```

New test cases:
- **RBAC enforcement**: Index graph with mixed public/private modules, enable RBAC, assert hub diagram contains only public module labels
- **No file paths in hub output**: Assert hub Mermaid output contains zero `/` path separators
- **Policy annotation**: When `policy-lint` is active, assert boundary violations appear as styled edges on the diagram

---

## 5. RBAC Data Flow

```text
                    CLI (local)                          Hub (custom mode)
                    ──────────                           ─────────────────
storage.all_nodes() ──┐                    storage.all_nodes() ──┐
storage.all_edges() ──┤                    storage.all_edges() ──┤
                      ▼                                          ▼
              RbacGuard::visible()                       RbacGuard::visible()
              (filter nodes/edges)                       (filter nodes/edges)
                      │                                          │
                      ▼                                          ▼
              build_modules()                            build_modules()
              (Louvain clustering)                       (Louvain clustering)
                      │                                          │
                      ▼                                          ▼
         ┌── render_mermaid(LOD 0) ──┐             render_mermaid(LOD 1)
         ├── render_mermaid(LOD 1) ──┤              (modules + edges only)
         ├── render_mermaid(LOD 2) ──┤                       │
         │   (per-module files)      │                       ▼
         ▼                           ▼               text/plain response
    .mmd files              WEAVE_REPORT.md           (no file paths,
                                                       no symbols)
```

Key invariant: `render_mermaid()` never touches `Storage` or `RbacGuard`. It receives already-filtered `&[Module]` and emits text. The RBAC boundary sits between storage and module-building, owned by the caller.

---

## 6. Configuration Changes

### 6.1 `.weave/config.toml`

```toml
[report]
# "mermaid" (default): .mmd files + optional HTML
# "canvas":  legacy JSON Canvas (deprecated, kept for Obsidian users)
# "all":     both formats
format = "mermaid"
auto_open = false

[viz]
mode = "static"  # "static" or "server"
```

### 6.2 CLI Flags (Backward Compatibility)

```text
weave report --format mermaid     # explicit (default)
weave report --format canvas      # legacy
weave report --format all         # both
weave report --html               # render HTML viewer from .mmd
```

---

## 7. Mermaid Output Examples

### LOD 0 — Repo Overview (CLI)

```text
graph TD
    repo-local["local<br/>142 files, 3 modules"]
```

### LOD 1 — Module Map (CLI + Hub)

```text
graph TD
    module-0["auth<br/>12 files"]
    module-1["storage<br/>8 files"]
    module-2["parser<br/>15 files"]
    module-0 -->|"23"| module-1
    module-1 -->|"7"| module-2
```

This is the **maximum detail level** the hub ever serves. No file paths, no symbols — just module labels, file counts, and inter-module coupling weights.

### LOD 1 with RBAC (Custom Mode Hub)

Given RBAC hides the `auth` module:

```text
graph TD
    module-1["storage<br/>8 files"]
    module-2["parser<br/>15 files"]
    module-1 -->|"7"| module-2
```

Hidden modules are **dropped entirely** — not masked, not placeholder-replaced. The diagram shows only what RBAC permits.

### LOD 1 with Policy Violations

```text
graph TD
    module-0["auth<br/>12 files"]
    module-1["storage<br/>8 files"]
    module-2["parser<br/>15 files"]
    module-0 -->|"23"| module-1
    module-1 -.->|"⚠ disallow"| module-2
    linkStyle 1 stroke:red
```

### LOD 2 — Single Module Detail (CLI Only)

```text
graph TD
    subgraph auth ["auth (12 files)"]
        f0["src/auth/mod.rs"]
        f1["src/auth/token.rs"]
        f2["src/auth/session.rs"]
        f0 --> f1
        f0 --> f2
    end
```

### Mesh — Multi-Repo (Hub, LOD 1 Only)

```text
graph LR
    subgraph repo-a ["repo-a"]
        a-mod-0["auth<br/>12 files"]
        a-mod-1["storage<br/>8 files"]
    end
    subgraph repo-b ["repo-b"]
        b-mod-0["api<br/>6 files"]
        b-mod-1["models<br/>4 files"]
    end
```

---

## 8. Benefits

| Dimension | JSON Canvas (Current) | Mermaid (Proposed) |
|:---|:---|:---|
| **GitHub rendering** | ❌ Raw JSON | ✅ Native in markdown |
| **PR diff review** | Opaque JSON changes | Human-readable text diffs |
| **Dependencies** | `serde`, `serde_json` in hub | None (plain text) |
| **Viewer complexity** | Custom canvas JS renderer | Mermaid.js (standard, maintained) |
| **Obsidian** | ✅ Native | ✅ Native (Mermaid blocks render) |
| **CI artifacts** | Requires viewer to read | Copy-paste into any markdown |
| **Code duplication** | `report.rs` + `hub/canvas.rs` both build `CanvasNode` | One shared `render_mermaid()` |
| **RBAC compliance** | ❌ Hub bypasses (Invariant 7 gap) | ✅ Pre-filtered data only |
| **Detail leakage** | File paths visible in hub | ❌ Hub is LOD 1 only |

---

## 9. Migration Path

### Phase 1: Add Mermaid Output (Non-Breaking)

- Add `render_mermaid.rs` to `weave-graph-core`
- `weave report` emits `.mmd` alongside `.canvas` when `format = "all"`
- Hub `/canvas` endpoint gains `Accept: text/plain` → Mermaid, `Accept: application/json` → legacy
- Wire RBAC filter in hub `build_module_canvas()` (fixes pre-existing Invariant 7 gap)

### Phase 2: Default Switch

- `[report] format` default changes from `"canvas"` to `"mermaid"`
- `.canvas` output gated behind `format = "canvas"` or `format = "all"`
- Hub endpoint keeps `/repos/{id}/canvas` path for backward compatibility
- Default `Accept` response switches from JSON Canvas to Mermaid text
- Release notes document the change

### Phase 3: Deprecation

- `CanvasNode`/`Canvas` structs in `report.rs` marked `#[deprecated]`
- Hub `hub-canvas` feature drops `serde_json` dependency
- `canvas_viewer.html` replaced with Mermaid-based viewer

---

## 10. Resolved Decisions

- **Hub endpoint path**: Keep `/repos/{id}/canvas` for backward compatibility. Use `Accept` header content negotiation — `text/plain` returns Mermaid (new default), `application/json` returns legacy JSON Canvas.
- **Mermaid.js embedding**: Embed `mermaid.min.js` (~200KB) into the binary via `include_str!` for offline `weave viz` and `weave report --html`. Only included in the HTML output when `[report] format` is `"mermaid"` or `"all"` — keeps the offline-first invariant (Core Invariant 5), no CDN fetch, no network dependency.
- **Renderer placement**: `render_mermaid.rs` lives in `weave-graph-core`. Zero new dependencies (pure `String` output from `&[Module]` + edges). Both CLI (`report.rs`) and Hub (`canvas.rs`) call the same shared implementation — eliminates the current duplication where both crates independently build `CanvasNode` structures.
