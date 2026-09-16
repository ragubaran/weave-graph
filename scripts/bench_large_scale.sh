#!/usr/bin/env bash
# Large-scale performance benchmark for weave-graph.
# Tests:
#   1. Large repository indexing throughput (SQLite + vector)
#   2. Vector search latency and recall
#   3. Peak RSS during large-scale indexing
set -euo pipefail

cd "$(dirname "$0")/.."

# Default: 1M symbols (~100 MB source corpus)
SYMBOL_COUNT=${SYMBOL_COUNT:-1000000}
PER_FILE=${PER_FILE:-500}
SEARCH_QUERIES=${SEARCH_QUERIES:-100}
RSS_BUDGET=$((500 * 1024 * 1024))  # 500 MiB for large-scale test
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

echo "============================================"
echo "  Large-Scale Performance Benchmark"
echo "============================================"
echo "  Symbols:     $SYMBOL_COUNT"
echo "  Per file:    $PER_FILE"
echo "  Search qrys: $SEARCH_QUERIES"
echo "  Work dir:    $WORK"
echo ""

# ---------------------------------------------------------------------------
# Phase 1: Generate corpus
# ---------------------------------------------------------------------------
echo "--- Phase 1: Generating $SYMBOL_COUNT symbols ---"
mkdir -p "$WORK/src"
files=$(( (SYMBOL_COUNT + PER_FILE - 1) / PER_FILE ))
echo "  Files: $files"

for file in $(seq 0 $((files - 1))); do
    out="$WORK/src/file_${file}.rs"
    : > "$out"
    for symbol in $(seq 0 $((PER_FILE - 1))); do
        id=$((file * PER_FILE + symbol))
        [ "$id" -lt "$SYMBOL_COUNT" ] || break
        if [ "$symbol" -gt 0 ] && [ $((symbol % 10)) -eq 0 ]; then
            prev=$((id - 1))
            printf 'pub fn symbol_%d() { symbol_%d(); }\n' "$id" "$prev" >> "$out"
        else
            printf 'pub fn symbol_%d() {}\n' "$id" >> "$out"
        fi
    done
done

repo_size=$(du -sh "$WORK/src" | awk '{print $1}')
echo "  Corpus size: $repo_size"

# ---------------------------------------------------------------------------
# Phase 2: Build core CLI and index
# ---------------------------------------------------------------------------
echo ""
echo "--- Phase 2: Indexing in SQLite ---"

cargo build --release -q -p weave-graph-cli --no-default-features --bin weave
BIN="$(pwd)/target/release/weave"

(cd "$WORK" && "$BIN" init --mode single >/dev/null 2>&1)

if [ "$(uname)" = Darwin ]; then
    (cd "$WORK" && /usr/bin/time -l "$BIN" index 2>"$WORK/index.time" >/dev/null)
    rss=$(awk '/maximum resident set size/ {print int($1 / 1024 / 1024)}' "$WORK/index.time")
else
    (cd "$WORK" && /usr/bin/time -v "$BIN" index 2>"$WORK/index.time" >/dev/null)
    rss=$(awk -F: '/Maximum resident set size/ {gsub(/ /, "", $2); print int($2 / 1024)}' "$WORK/index.time")
fi
index_time=$(awk '/real/ {print $2}' "$WORK/index.time" 2>/dev/null || echo "N/A")
echo "  Index time: ${index_time}s"
echo "  Peak RSS:   ${rss} MiB (budget: $((RSS_BUDGET / 1024 / 1024)) MiB)"

if [ "$rss" -le $((RSS_BUDGET / 1024 / 1024)) ]; then
    echo "  ✓ RSS within budget"
else
    echo "  ✗ RSS exceeds budget!"
fi

# ---------------------------------------------------------------------------
# Phase 3: Vector indexing
# ---------------------------------------------------------------------------
echo ""
echo "--- Phase 3: Vector indexing ---"

cargo build --release -q -p weave-graph-cli --no-default-features --features vector --bin weave
BIN_VEC="$(pwd)/target/release/weave"

if [ "$(uname)" = Darwin ]; then
    (cd "$WORK" && /usr/bin/time -l "$BIN_VEC" index 2>"$WORK/vec_index.time" >/dev/null)
    vec_rss=$(awk '/maximum resident set size/ {print int($1 / 1024 / 1024)}' "$WORK/vec_index.time")
else
    (cd "$WORK" && /usr/bin/time -v "$BIN_VEC" index 2>"$WORK/vec_index.time" >/dev/null)
    vec_rss=$(awk -F: '/Maximum resident set size/ {gsub(/ /, "", $2); print int($2 / 1024)}' "$WORK/vec_index.time")
fi
vec_time=$(awk '/real/ {print $2}' "$WORK/vec_index.time" 2>/dev/null || echo "N/A")
echo "  Vector index time: ${vec_time}s"
echo "  Peak RSS:          ${vec_rss} MiB"

# ---------------------------------------------------------------------------
# Phase 4: Vector search performance
# ---------------------------------------------------------------------------
echo ""
echo "--- Phase 4: Vector search performance ---"

echo "  Running $SEARCH_QUERIES vector search queries..."

search_terms=()
for _ in $(seq 1 "$SEARCH_QUERIES"); do
    file=$((RANDOM % files))
    sym=$((RANDOM % PER_FILE))
    id=$((file * PER_FILE + sym))
    [ "$id" -lt "$SYMBOL_COUNT" ] || continue
    search_terms+=("symbol_${id}")
done

total_time=0
hits=0
count=0
for term in "${search_terms[@]:0:50}"; do
    count=$((count + 1))
    start=$(date +%s%N)
    if (cd "$WORK" && "$BIN_VEC" search "$term" >/dev/null 2>&1); then
        hits=$((hits + 1))
    fi
    end=$(date +%s%N)
    elapsed=$(( (end - start) / 1000000 ))
    total_time=$((total_time + elapsed))
done

avg_latency=$(( total_time / (count > 0 ? count : 1) ))
echo "  Queries:     $count"
echo "  Hits:        $hits"
echo "  Avg latency: ${avg_latency}ms"

# ---------------------------------------------------------------------------
# Phase 5: Database size report
# ---------------------------------------------------------------------------
echo ""
echo "--- Phase 5: Database size ---"
db_size=$(du -sh "$WORK/.weave/graph.db" | awk '{print $1}')
echo "  SQLite DB:   $db_size"
wal_size=$(du -sh "$WORK/.weave/graph.db-wal" 2>/dev/null | awk '{print $1}' || echo "0")
echo "  WAL:         $wal_size"
total_db=$(du -sh "$WORK/.weave" | awk '{print $1}')
echo "  Total .weave: $total_db"

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo "============================================"
echo "  Benchmark Summary"
echo "============================================"
echo "  Corpus:       $SYMBOL_COUNT symbols in $files files ($repo_size)"
echo "  Index time:   ${index_time}s"
echo "  Index RSS:    ${rss} MiB"
echo "  Vector time:  ${vec_time}s"
echo "  Vector RSS:   ${vec_rss} MiB"
echo "  Search avg:   ${avg_latency}ms"
echo "  DB size:      $total_db"
echo "============================================"