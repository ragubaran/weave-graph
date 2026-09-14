# `slm` Feature Specification: Local Intelligence Layer

> **Document ID**: `slm-spec.md`
> **Status**: Historical intent-router specification. [plan.md](plan.md#6-approved-phase-4-product-decisions) supersedes its model-use and profile decisions; [impl.md](impl.md) records actual implementation status. CLI examples and latency budgets below are design illustrations unless verified against the current binary.
> **Topic**: Scope, boundary, CLI surface, performance budget, and 2027 refinement targets for the optional `slm` Cargo feature
> **Project**: `weave-graph` (CLI: `weave`)
> **Source material**: earlier SLM architecture research (now summarized as an unapproved idea in [proposal.md](proposal.md)), `plan.md` §0.2 (feature matrix)

---

## 1. Framing: What This Feature Is For

`weave-graph`'s core mission is **graph knowledge, served to a reasoning console.** The deterministic engine builds the graph; an LLM — frontier model, coding agent, or local SLM — consumes it. `plan.md` Architecture Principle guarantees the engine never requires a model to build, index, or query anything.

The `slm` feature does not change that. It adds **one narrow capability: a local model that turns human phrasing into an exact graph query, so a person at a terminal gets graph knowledge without paying for a cloud subscription or learning query syntax.**

### 1.1 The Boundary That Matters Most

**`slm` is for humans at a console. It must never sit between an AI coding agent and the MCP server.**

Agents like Claude Code, Cursor, and Copilot already emit exact structured tool calls — `weave_trace_calls(symbol, depth)` needs no translation. Inserting a 0.5B model in that path would add latency, add a paraphrase step that can only lose fidelity, and degrade a working interface. The MCP server stays purely deterministic regardless of whether `slm` is compiled in.

```text
┌────────────────────────────────────────────────────────────────────────────┐
│ PATH A — AI agent (default, no slm)                                        │
│   Agent ──exact MCP tool call──► weave-graph-core ──► subgraph ──► Agent    │
│   Deterministic end to end. slm is NOT in this path, ever.                 │
├────────────────────────────────────────────────────────────────────────────┤
│ PATH B — Human at terminal (feature: slm)                                  │
│   Human ──"who calls JWT verify?"──► local SLM ──► exact tool call         │
│                                          │                                 │
│                                          ▼                                 │
│                  weave-graph-core ──► subgraph ──► rendered answer         │
│   The SLM only translates intent. It never invents graph facts.            │
└────────────────────────────────────────────────────────────────────────────┘
```

### 1.2 Grounding Invariant (Non-Negotiable)

**The model may select tools and parameters. It may never author graph facts.** Every symbol, edge, file path, and line number in any `slm`-mediated answer must come from the deterministic engine. If the model names a symbol that does not exist in the index, the CLI reports that the symbol was not found — it never passes an unverified name through as though the graph confirmed it.

This is what separates `weave-graph` from LLM-extracted knowledge graphs, which `paper.md` identifies as the failure mode deterministic parsing exists to avoid. Retaining that guarantee while adding natural-language convenience is the entire design constraint of this feature.

---

## 2. In Scope: Runtime Capabilities

### 2.1 `weave ask "<question>"` — Natural-Language Query Routing

The primary surface. Translates intent into an exact tool call, executes it deterministically, renders the result.

```text
$ weave ask "who calls JWT session verification?"
  → routed: get_symbol_callers(symbol="verifyJWTSession", depth=2)   [18ms]

  verifyJWTSession  services/auth/token.rs:L42-L78
  ├── handleLogin           services/auth/routes.rs:L112
  ├── refreshSession        services/auth/routes.rs:L140
  └── requireAuth           middleware/guard.rs:L23
        └── (7 route handlers — `weave ask --expand requireAuth`)
```

*   **Routing transparency is mandatory.** The resolved tool call is always shown, so the user can verify the interpretation and learn the direct syntax. A silent black box would make a wrong interpretation indistinguishable from a wrong graph.
*   `--dry-run` prints the routed call without executing it.
*   `--json` emits the structured result for scripting.
*   On ambiguity, present the top candidate interpretations and let the user choose rather than guessing.
*   On routing failure, fall back to deterministic fuzzy symbol matching (already required by the core) rather than erroring out.

### 2.2 `weave slm pull <model>` / `weave slm list` — Model Management

No weights are bundled in the binary. Models are fetched on request into `$XDG_CACHE_HOME/weave/models/`, and the same network-filesystem guard from `plan.md` §1.4 applies to that path.

| Model | Params | RAM (Q4_K_M) | Role |
| :--- | :--- | :--- | :--- |
| Qwen2.5-Coder-0.5B | 0.5B | ~380 MB | **Default.** Tool routing only; runs anywhere. |
| Qwen2.5-Coder-1.5B | 1.5B | ~1.1 GB | Better entity extraction from prose. |
| Llama-3.2-3B | 3.2B | ~2.2 GB | Doc summarization, changelog prose. |
| Qwen2.5-Coder-7B | 7.6B | ~4.5 GB | Deep local reasoning; opt-in only. |

Verify a checksum on download. Never auto-upgrade a model underneath a user — a silent model swap changes routing behavior invisibly.

### 2.3 Prose Rule Extraction from ADRs *(additive on `docs`)*

`docs` already parses wikilinks, headings, and frontmatter deterministically with `pulldown-cmark` — that needs no model and must keep working without one.

What `slm` adds is extracting **free-form architectural rules** that tokenization cannot reach: *"services must not call the database directly"* → a candidate `policy` edge between module nodes.

*   Extracted rules are written as **candidates requiring confirmation** (`weave slm review-rules`), never as authoritative graph facts. This preserves §1.2 while still capturing knowledge that only exists as prose.
*   Confirmed rules feed the `policy-lint` feature when both are enabled — turning a paragraph in an ADR into an enforceable CI check is the highest-leverage thing this feature can do for a team.

### 2.4 `weave journal [--since <ref>]` — Dev Journal & Changelog Synthesis

Combines the git diff with the graph delta (both already computed by the incremental indexer) and synthesizes a structured changelog: what changed, which symbols were affected, what the blast radius was, which docs reference the changed code.

The graph delta is deterministic; the model only supplies prose framing. Symbol names and line ranges come from the index.

### 2.5 `weave slm doctor` — Self-Check *(Required)*

Non-trivial routing logic ships with one runnable check. A fixed set of held-out prompts is run through the currently-loaded model, asserting correct tool and parameter selection.

```text
$ weave slm doctor
  model: qwen2.5-coder-0.5b-q4_k_m
  tool selection      14/15 ✓   (target > 95%)
  param grounding     15/15 ✓   (target > 98%: params must exist in index)
  TTFT p50               61ms ✓  (target < 100ms)
  TTFT p95              104ms ✓
  → PASS
```

CPU-only, runs in seconds, changes nothing. Suitable for CI and for verifying a model after `weave slm pull`.

---

## 3. Out of Scope: Offline Training Workflow

The earlier SLM sketch described a fine-tuning pipeline. **It is not part of the `slm` Cargo feature and no part of it compiles into the `weave` binary.** Conflating the two would imply that enabling a CLI flag pulls in a GPU training stack.

| | `weave slm doctor` (§2.5, **in scope**) | Self-improvement loop ([proposal.md](proposal.md), **out of scope**) |
| :--- | :--- | :--- |
| **Purpose** | Verify the loaded model still routes correctly | Produce a new, codebase-tuned model |
| **Effect** | Asserts. Changes nothing. | Writes new GGUF weights |
| **Hardware** | CPU, seconds | GPU (6–8GB VRAM), ~30 min |
| **Runs** | Anytime — post-install, CI, on demand | Occasionally, by a maintainer |
| **Lives in** | `weave-graph-cli`, behind `--features slm` | Separate Python/Unsloth tooling in `tools/slm-tune/` |

The training loop's genuine insight is worth preserving in that separate tooling: **the graph is an objective oracle.** A generated Q&A pair whose call path does not exist in the AST is automatically a negative example, so labeling requires no human. That is a real advantage of building a tuner on top of a deterministic graph — it just isn't a CLI feature.

The earlier three-tier spectrum (Zero-LLM / SLM / frontier) is guidance, not a shipped runtime capability; use the approved profiles in [plan.md](plan.md) instead.

---

## 4. Performance Budget

The engine's `<5ms` deterministic query latency is a headline guarantee. Adding a model must not silently erode it.

### 4.1 Hard Budgets

| Path | Budget | Notes |
| :--- | :--- | :--- |
| Deterministic query (`weave query`, MCP) | **`<5ms`, unchanged** | `slm` being compiled in must not add measurable overhead to this path. Non-negotiable. |
| `weave ask` routing (SLM inference) | **`<100ms` TTFT p50, `<250ms` p95** | 0.5B Q4_K_M on a consumer quad-core. |
| `weave ask` graph execution | `<5ms` | Same engine, same guarantee. |
| **`weave ask` end-to-end** | **`<400ms` p95** | Still an order of magnitude below a frontier round-trip. |
| Idle RSS added when `slm` compiled but unused | **`0 MB`** | Model loads lazily on first `weave ask`, never at startup. |
| Idle RSS with 0.5B model resident | `~380 MB` on top of core's `<80MB` | Documented cost of opting in. |

### 4.2 Required Performance Properties

*   **Lazy load, always.** `weave index`, `weave query`, and `weave serve --mcp` must never load model weights. A user who compiled `slm` but is running an index must pay nothing for it.
*   **No model on the MCP path.** Restates §1.1 as a performance property: agent queries stay at engine latency.
*   **Warm-process reuse.** Repeated `weave ask` calls in one session should not reload weights each time; a short-lived resident process or explicit `weave ask --repl` avoids paying model load per question.
*   **Graceful degradation.** If the model is missing, corrupt, or too large for available RAM, `weave ask` reports it and falls back to deterministic fuzzy matching. It never hangs, never OOMs the machine, never silently swaps.

### 4.3 Benchmark Additions (Phase 1 Exit Gate Discipline)

Per `performance_compare.md`'s measurement policy, **every figure in §4.1 is a Target SLO, not a measurement.** The `slm` feature adds to `benches/`:

*   `benches/slm_routing.rs` — TTFT and end-to-end `weave ask` latency across the model spectrum.
*   `benches/slm_accuracy.rs` — tool-selection and parameter-grounding rates against the held-out prompt set.
*   A regression assertion that compiling `--features slm` does not change deterministic query latency beyond measurement noise.

No `slm` performance claim may be published until these produce reproducible `criterion` output.

---

## 5. 2027 Refinement Targets

The feature is aimed at a 3-year horizon, so the design must survive a fast-moving local-model ecosystem.

1.  **Model-agnostic behind a trait.** Define an `IntentRouter` trait — the current `llama.cpp`/GGUF path is one implementation. Local inference runtimes have churned repeatedly (GGML → GGUF, ONNX, MLX, and whatever follows); the routing logic must not be welded to today's loader. Consistent with `plan.md` §0.4's provider-trait discipline.
2.  **Constrained decoding over free generation.** Tool calls should be produced with grammar-constrained/structured decoding so the model *cannot* emit a malformed call. This turns "parse the model's JSON and hope" into a structural guarantee, and it is the single highest-value refinement available — accuracy targets become far easier to hit when invalid output is unrepresentable.
3.  **Apple Silicon / MLX path.** A large share of the developer audience is on ARM Macs where MLX substantially outperforms generic CPU inference. Worth a dedicated backend behind the `IntentRouter` trait.
4.  **Index-aware parameter grounding.** Before dispatching, validate that every symbol the model named exists in the index; on a near-miss, correct it against the symbol table rather than querying a nonexistent name. This makes §1.2 mechanical instead of aspirational.
5.  **Router accuracy telemetry, local only.** Record routing corrections the user makes (`--dry-run` rejections, disambiguation choices) to a local file. That corpus is exactly what the out-of-scope tuner in §3 consumes — closing the loop without any data egress.
6.  **Explicit non-goal: local code generation.** `slm` does not write code. Feature-design assistance may make suggestions with grounded evidence when explicitly requested, but autonomous multi-file modification is outside the approved model scope.

---

## 6. Summary

| Aspect | Decision |
| :--- | :--- |
| **Purpose** | Natural-language access to graph knowledge for a human at a terminal, with zero cloud cost or egress |
| **Never** | On the MCP/agent path; authoring graph facts; generating code; required by any tier |
| **CLI surface** | `weave ask`, `weave slm pull|list|doctor|review-rules`, `weave journal` |
| **Default model** | Qwen2.5-Coder-0.5B Q4_K_M (~380 MB), lazily loaded, never bundled |
| **Core guarantee** | Deterministic paths stay at `<5ms` and `0 MB` added idle cost; the engine remains fully functional with no model present |
| **Training pipeline** | Out of scope — separate `tools/slm-tune/`, GPU, occasional, maintainer-run |
| **Key refinement** | Constrained decoding + index-aware grounding, behind a swappable `IntentRouter` trait |

---
*Authored 2026-09-09 as the initial scoping specification for `plan.md`'s `slm` feature; later Phase 4 decisions supersede its broader model-use assumptions.*
