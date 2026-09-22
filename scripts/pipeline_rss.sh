#!/usr/bin/env bash
# Measure the complete CLI indexing pipeline on a deterministic corpus,
# plus a long-lived MCP session's peak RSS and per-query latency against
# the resulting 500k-symbol database — the "full indexing/MCP RSS and
# latency" measurement `impl.md`'s own verification-gaps table names as
# still needing "the complete process, not only SQLite/CSR" (`mem_500k`'s
# own scope). Two phases, one corpus, one binary.
set -euo pipefail

cd "$(dirname "$0")/.."
SYMBOLS=${SYMBOLS:-500000}
PER_FILE=${PER_FILE:-100}
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

cargo build --release -q -p weave-graph-cli --no-default-features
BIN="$(pwd)/target/release/weave"
mkdir -p "$WORK/src"
files=$(( (SYMBOLS + PER_FILE - 1) / PER_FILE ))
for file in $(seq 0 $((files - 1))); do
    out="$WORK/src/file_${file}.rs"
    : > "$out"
    for symbol in $(seq 0 $((PER_FILE - 1))); do
        id=$((file * PER_FILE + symbol))
        [ "$id" -lt "$SYMBOLS" ] || break
        printf 'pub fn symbol_%d() {}\n' "$id" >> "$out"
    done
done
(cd "$WORK" && "$BIN" init >/dev/null)
time_output="$WORK/index.time"

if [ "$(uname)" = Darwin ]; then
    if ! (cd "$WORK" && /usr/bin/time -l "$BIN" index >/dev/null) 2>"$time_output"; then
        cat "$time_output" >&2
        exit 1
    fi
    rss=$(awk '/maximum resident set size/ {print int($1 / 1024 / 1024)}' "$time_output")
else
    if ! (cd "$WORK" && /usr/bin/time -v "$BIN" index >/dev/null) 2>"$time_output"; then
        cat "$time_output" >&2
        exit 1
    fi
    rss=$(awk -F: '/Maximum resident set size/ {gsub(/ /, "", $2); print int($2 / 1024)}' "$time_output")
fi
[ -n "$rss" ] || { echo "peak RSS unavailable" >&2; exit 2; }
echo "pipeline symbols=$SYMBOLS files=$files peak_rss_mib=$rss budget_mib=80"

# --- Phase 2: long-lived MCP session against the freshly-indexed DB ---
# Same request-count/min-of-N idiom `feature_isolation.sh`'s own
# `latency_us` already uses, against the real 500k-symbol database this
# script just built (not a two-symbol fixture) — a full `initialize` +
# 200 `tools/call` session, timed end-to-end, peak RSS of that one process.
mcp_input="$WORK/mcp_input.jsonl"
{
    printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}\n'
    printf '{"jsonrpc":"2.0","method":"notifications/initialized"}\n'
    i=2
    while [ "$i" -lt 202 ]; do
        printf '{"jsonrpc":"2.0","id":%d,"method":"tools/call","params":{"name":"weave_impact_radius","arguments":{"symbol":"symbol_0"}}}\n' "$i"
        i=$((i + 1))
    done
} > "$mcp_input"

mcp_time_output="$WORK/mcp.time"
mcp_wall_start=$(date +%s%N)
if [ "$(uname)" = Darwin ]; then
    if ! (cd "$WORK" && /usr/bin/time -l "$BIN" serve --mcp < "$mcp_input" >/dev/null) 2>"$mcp_time_output"; then
        cat "$mcp_time_output" >&2
        exit 1
    fi
    mcp_rss=$(awk '/maximum resident set size/ {print int($1 / 1024 / 1024)}' "$mcp_time_output")
else
    if ! (cd "$WORK" && /usr/bin/time -v "$BIN" serve --mcp < "$mcp_input" >/dev/null) 2>"$mcp_time_output"; then
        cat "$mcp_time_output" >&2
        exit 1
    fi
    mcp_rss=$(awk -F: '/Maximum resident set size/ {gsub(/ /, "", $2); print int($2 / 1024)}' "$mcp_time_output")
fi
mcp_wall_end=$(date +%s%N)
mcp_wall_ms=$(( (mcp_wall_end - mcp_wall_start) / 1000000 ))
[ -n "$mcp_rss" ] || { echo "MCP session peak RSS unavailable" >&2; exit 2; }
mcp_per_query_ms=$(( mcp_wall_ms / 200 ))
echo "mcp_session symbols=$SYMBOLS queries=200 peak_rss_mib=$mcp_rss total_wall_ms=$mcp_wall_ms avg_query_ms=$mcp_per_query_ms budget_mib=80"

[ "$rss" -le 80 ] && [ "$mcp_rss" -le 80 ]
