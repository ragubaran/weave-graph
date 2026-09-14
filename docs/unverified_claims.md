# Unverified or deferred product claims

This internal register captures capabilities and measurements that must not be
advertised as shipped or release-certified until the implementation and an
automated, reproducible verification gate exist. User-facing pages in
`docs/product/` intentionally omit these claims.

## Deferred capabilities

- Learned BGE semantic embeddings, including a portable Intel macOS runtime,
  explicit model installer, complete embedding fingerprint, and quality tests.
- Approximate-nearest-neighbour (ANN) indexing, binary-ANN performance, and
  deterministic lexical/vector hybrid fusion.
- A separate Turso-only CLI binary or runtime backend switching. The current
  CLI links SQLite and does not safely exercise both native SQLite libraries
  in one process.
- Cross-repository or mesh policy-lint HTTP endpoints and path-scoped RBAC or
  IdP group-to-capability mappings.
- Per-request authentication for MCP stdio, rather than one process identity.
- Vision ingestion, local model fine-tuning, autonomous code modification, and
  background generative assistance.

## Measurements not yet release gates

- Whole-pipeline peak RSS for indexing 500k symbols under the 80 MB target.
- Reproducible cold, warm, incremental, rename/delete, search and caller
  latency baselines with p50/p95 reporting.
- Per-crate 90% coverage and a passing full feature/release matrix.
- Extended-language, Basic, vector and SLM package/RAM budgets. The <15 MB
  target applies only to the explicitly built core (`--no-default-features`).
- Semantic recall, MRR/nDCG, quantization loss, ANN throughput, or model
  startup/first-token measurements.

See [issues.md](issues.md) for status and [impl.md](impl.md) for task-level
acceptance gates. This file is not evidence that any listed item is complete.
