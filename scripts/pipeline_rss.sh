#!/usr/bin/env bash
# Measure the complete CLI indexing pipeline on a deterministic corpus.
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
[ "$rss" -le 80 ]
