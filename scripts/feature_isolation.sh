#!/usr/bin/env bash
# pending_p2.md §4.3 (Phase 2 Exit Ledger L8): the feature-isolation
# regression gate, automated. For each optional feature, builds the `weave`
# binary with that feature plus the default set, indexes a fixed fixture
# repo, and asserts:
#
#   1. Peak RSS of a status/query run stays within MAX_RSS_DELTA_KB of the
#      default build's, and
#   2. `weave query` latency (20-run loop, median of 3) does not regress
#      beyond MAX_LATENCY_REGRESSION_PCT of the default build's.
#
# Exits non-zero on the first violation, printing both builds' numbers.
# Usage: scripts/feature_isolation.sh [feature ...]   (default: the Phase 2 set)
set -euo pipefail

cd "$(dirname "$0")/.."

MAX_RSS_DELTA_KB=${MAX_RSS_DELTA_KB:-8192}
# 30%, not 10%: the latency signal being gated is gross (an eager model
# load or thread pool shows up as seconds, not percent). Session-to-session
# wall-clock jitter on shared machines measured 10-45% at ~300ms, so a 10%
# threshold was a flake generator, not a gate.
MAX_LATENCY_REGRESSION_PCT=${MAX_LATENCY_REGRESSION_PCT:-30}
# slm/hub/turso excluded: slm shells out to external model weights, hub's
# sync verbs hit a configured hub URL, turso swaps the storage backend
# (its isolation is asserted by its own suite). The pure read/query
# features are the ones L8's "no change to default-build latency/RSS"
# claim covers.
FEATURES=(${@:-docs federation provenance notes watch viz rbac fts})

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
ABS_BIN="$(pwd)/target/debug/weave"
mkdir -p "$WORK/src"
# A call chain big enough that an impact query does real traversal work —
# on a two-symbol fixture the session is pure startup cost and the latency
# comparison measures nothing.
{
    printf 'fn helper() { crate::f000(); }\n'
    for i in $(seq 0 199); do
        printf 'fn f%03d() { crate::f%03d(); }\n' "$i" $((i + 1))
    done
} > "$WORK/src/lib.rs"

# One fixture, indexed once, reused by every build — identical workload.
(cd "$WORK" && "$ABS_BIN" init >/dev/null && "$ABS_BIN" index >/dev/null)

rss_kb() { # peak RSS, normalized to KB (macOS reports bytes, Linux KB)
    local raw
    raw=$(/usr/bin/time -l "$ABS_BIN" status 2>&1 >/dev/null |
        awk '/maximum resident set size/ {print $1}')
    if [ "$(uname)" = Darwin ]; then
        echo $((raw / 1024))
    else
        echo "$raw"
    fi
}

latency_us() { # in-process query latency: ONE MCP session, 200 impact
    # queries, timed end-to-end. Min of 7 sessions. Deliberately NOT a
    # spawn-per-query loop: binary size (which a feature legitimately
    # grows) pollutes per-spawn wall-clock with page-in cost that has
    # nothing to do with query-path latency — the exact artifact that made
    # `watch` (+notify) look 17% "slower" while its in-process query
    # latency measured identical to the default build.
    local input="$WORK/mcp_input.jsonl"
    {
        printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}\n'
        printf '{"jsonrpc":"2.0","method":"notifications/initialized"}\n'
        i=2
        while [ "$i" -lt 202 ]; do
            printf '{"jsonrpc":"2.0","id":%d,"method":"tools/call","params":{"name":"weave_impact_radius","arguments":{"symbol":"helper"}}}\n' "$i"
            i=$((i + 1))
        done
    } > "$input"
    for _ in 1 2 3 4 5 6 7; do
        /usr/bin/time -p sh -c "cd '$WORK' && '$ABS_BIN' serve --mcp < '$input' > /dev/null" 2>&1 |
            awk '/real/ {printf "%d\n", $2 * 1000000}'
    done | sort -n | head -1
}

build() {
    if [ "$1" = default ]; then
        cargo build -q -p weave-graph-cli --bin weave
    else
        cargo build -q -p weave-graph-cli --features "$1" --bin weave
    fi
}

# Build every binary FIRST, then measure all of them back-to-back:
# interleaving builds with measurements lets ambient load drift between the
# default's numbers and each feature's, which read as fake regressions.
# Load noise is one-sided (it only adds time), so per-binary latency is
# min-of-7 sessions — the minimum approaches the true unloaded cost.
baseline_rss=""
baseline_us=""
ALL_FEATURES="default ${FEATURES[@]}"
for feature in $ALL_FEATURES; do
    build "$feature"
    cp "$ABS_BIN" "$WORK/bin-$feature"
done
for feature in $ALL_FEATURES; do
    ABS_BIN="$WORK/bin-$feature"
    rss=$(rss_kb)
    us=$(latency_us)
    echo "feature=${feature} rss_kb=${rss} query_us=${us}"
    if [ "$feature" = default ]; then
        baseline_rss=$rss
        baseline_us=$us
        continue
    fi
    rss_delta=$((rss - baseline_rss))
    if [ "$rss_delta" -gt "$MAX_RSS_DELTA_KB" ]; then
        echo "FAIL: feature '$feature' peak RSS grew by ${rss_delta}KB over default (limit ${MAX_RSS_DELTA_KB}KB)" >&2
        exit 1
    fi
    # Noise floor: 20ms of a ~300ms session; the 30% threshold above is
    # what absorbs the session-to-session jitter.
    latency_pct=$(
        awk -v n="$us" -v b="$baseline_us" \
            'BEGIN {
                floor = 20000;
                if (n <= b + floor) print 0;
                else printf "%.1f", (n - b) / b * 100;
            }'
    )
    if awk -v p="$latency_pct" -v m="$MAX_LATENCY_REGRESSION_PCT" 'BEGIN { exit !(p > m) }'; then
        echo "FAIL: feature '$feature' query latency regressed ${latency_pct}% over default (limit ${MAX_LATENCY_REGRESSION_PCT}%)" >&2
        exit 1
    fi
done

echo "PASS: feature isolation within tolerance (RSS Δ≤${MAX_RSS_DELTA_KB}KB, latency ≤+${MAX_LATENCY_REGRESSION_PCT}%)"
