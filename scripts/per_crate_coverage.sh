#!/usr/bin/env bash
# Per-crate line-coverage gate. CI's own coverage job
# (`cargo llvm-cov --workspace --all-targets --fail-under-lines 90`) is a
# single workspace-aggregate number — one crate well under 90% can hide
# behind others well over it. This runs the identical gate (same flags,
# default features, no --all-features — matching CI exactly) once per
# crate instead, so each one stands on its own.
#
# `weave-graph-python` is skipped here even though CI's own workspace
# command doesn't explicitly exclude it: its PyO3 extension-module build
# needs maturin's own linker setup, and on macOS specifically this fails
# to link `libpython` at test-binary link time (verified this session,
# `dyld: Library not loaded: @rpath/libpython3.8.dylib`) — a host-platform
# problem, not something this gate should mask by skipping the crate
# silently. Run its coverage via the dedicated `python-bindings` CI job's
# own Docker environment instead.
set -euo pipefail

cd "$(dirname "$0")/.."

THRESHOLD=${THRESHOLD:-90}
CRATES=(
    weave-graph-core
    weave-graph-parse
    weave-graph-store-sqlite
    weave-graph-store-turso
    weave-graph-mcp
    weave-graph-cli
    weave-graph-hub
    weave-graph-wasm
)

echo "============================================"
echo "  Per-Crate Line Coverage (>= ${THRESHOLD}%)"
echo "============================================"
echo ""

declare -a results=()
fail_count=0
for crate in "${CRATES[@]}"; do
    echo "--- $crate ---"
    if cargo llvm-cov -p "$crate" --all-targets --fail-under-lines "$THRESHOLD" --summary-only; then
        results+=("  ✓ $crate")
    else
        results+=("  ✗ $crate (below ${THRESHOLD}%)")
        fail_count=$((fail_count + 1))
    fi
    echo ""
done

echo "============================================"
echo "  Summary"
echo "============================================"
printf '%s\n' "${results[@]}"

if [ "$fail_count" -gt 0 ]; then
    echo ""
    echo "FAIL: $fail_count crate(s) below ${THRESHOLD}% line coverage" >&2
    exit 1
fi
echo ""
echo "PASS: every crate at or above ${THRESHOLD}% line coverage"
