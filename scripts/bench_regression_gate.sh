#!/usr/bin/env bash
# Turns criterion's baseline comparison into a real pass/fail gate.
# `ci.yml`'s own bench job comment names exactly this as missing: "criterion
# has no built-in hard-fail threshold flag ... needs a script parsing
# target/criterion/**/estimates.json (or critcmp) — not implemented yet."
# This is that script.
#
# Usage:
#   scripts/bench_regression_gate.sh save      # on main: save the baseline
#   scripts/bench_regression_gate.sh           # on a PR: compare and gate
#
# `THRESHOLD_PCT` (default 10) is the same magnitude ci.yml's docs already
# name ("gate regressions at 10%", AGENTS.md §3's benchmarking row).
set -euo pipefail

cd "$(dirname "$0")/.."

BASELINE=${BASELINE:-main}
THRESHOLD_PCT=${THRESHOLD_PCT:-10}
MODE=${1:-compare}

# Exact same 8 --bench targets ci.yml's bench job names — named targets
# only, never --benches: that also matches lib/bin targets, which run
# their #[test] fns through libtest and reject criterion's own
# --save-baseline/--baseline args.
BENCHES=(csr_memory parser_throughput sqlite_latency vector_recall turso_latency slm_routing slm_accuracy indexing_throughput)
BENCH_ARGS=()
for b in "${BENCHES[@]}"; do
    BENCH_ARGS+=(--bench "$b")
done

if [ "$MODE" = save ]; then
    cargo bench --workspace --all-features --exclude weave-graph-python \
        "${BENCH_ARGS[@]}" -- --save-baseline "$BASELINE"
    echo "Saved baseline '$BASELINE'."
    exit 0
fi

# `target/criterion` accumulates every group ever benchmarked locally, not
# just this run's — a marker file lets `find -newer` isolate only the
# `change/estimates.json` files this specific invocation just wrote,
# instead of re-judging stale results from an earlier ad-hoc run.
marker=$(mktemp)
cargo bench --workspace --all-features --exclude weave-graph-python \
    "${BENCH_ARGS[@]}" -- --baseline "$BASELINE"

regressed=()
while IFS= read -r -d '' file; do
    pct=$(jq '.mean.point_estimate * 100' "$file")
    name=$(dirname "$(dirname "$file")")
    name=${name#target/criterion/}
    if awk -v p="$pct" -v t="$THRESHOLD_PCT" 'BEGIN { exit !(p > t) }'; then
        regressed+=("$(printf '%s: +%.1f%%' "$name" "$pct")")
    fi
done < <(find target/criterion -path '*/change/estimates.json' -newer "$marker" -print0)
rm -f "$marker"

if [ "${#regressed[@]}" -gt 0 ]; then
    echo "FAIL: ${#regressed[@]} benchmark(s) regressed beyond ${THRESHOLD_PCT}% against baseline '$BASELINE':" >&2
    printf '  %s\n' "${regressed[@]}" >&2
    exit 1
fi
echo "PASS: no benchmark regressed beyond ${THRESHOLD_PCT}% against baseline '$BASELINE'"
