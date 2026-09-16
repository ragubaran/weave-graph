#!/usr/bin/env bash
# Comprehensive resource envelope verification script.
# Measures:
#   1. Core artifact size (< 15 MiB)
#   2. Feature builds (vector, extended) size (< 50 MiB)
#   3. 500k symbol indexing peak RSS in SQLite (< 80 MiB)
#   4. 500k symbol indexing peak RSS in Turso (< 80 MiB)
set -euo pipefail

cd "$(dirname "$0")/.."

CORE_BUDGET=$((15 * 1024 * 1024))       # 15 MiB
FEATURE_BUDGET=$((50 * 1024 * 1024))    # 50 MiB
RSS_BUDGET=$((80 * 1024 * 1024))        # 80 MiB

PASS=0
FAIL=0

pass() { PASS=$((PASS + 1)); echo "  ✓ $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  ✗ $1"; }

echo "============================================"
echo "  Resource Envelope Verification"
echo "============================================"
echo ""

# ---------------------------------------------------------------------------
# 1. Core artifact size (< 15 MiB)
# ---------------------------------------------------------------------------
echo "--- 1. Core artifact size (< 15 MiB) ---"
cargo build --release -q -p weave-graph-cli --no-default-features --bin weave
core_bytes=$(wc -c < target/release/weave | tr -d ' ')
core_mib=$(echo "scale=2; $core_bytes / 1048576" | bc)
echo "    core binary: ${core_bytes} bytes (${core_mib} MiB), budget: 15728640 bytes (15 MiB)"
if [ "$core_bytes" -le "$CORE_BUDGET" ]; then
    pass "core artifact ${core_mib} MiB <= 15 MiB"
else
    fail "core artifact ${core_mib} MiB exceeds 15 MiB budget"
fi

# ---------------------------------------------------------------------------
# 2. Feature builds (< 50 MiB)
# ---------------------------------------------------------------------------
echo ""
echo "--- 2. Feature builds (< 50 MiB) ---"

for profile_name in "vector" "lang-extended" "custom"; do
    features="$profile_name"
    cargo build --release -q -p weave-graph-cli --no-default-features --features "$features" --bin weave
    feat_bytes=$(wc -c < target/release/weave | tr -d ' ')
    feat_mib=$(echo "scale=2; $feat_bytes / 1048576" | bc)
    echo "    ${profile_name}: ${feat_bytes} bytes (${feat_mib} MiB), budget: 52428800 bytes (50 MiB)"
    if [ "$feat_bytes" -le "$FEATURE_BUDGET" ]; then
        pass "${profile_name} artifact ${feat_mib} MiB <= 50 MiB"
    else
        fail "${profile_name} artifact ${feat_mib} MiB exceeds 50 MiB budget"
    fi
done

# ---------------------------------------------------------------------------
# 3. 500k symbol indexing peak RSS in SQLite (< 80 MiB)
# ---------------------------------------------------------------------------
echo ""
echo "--- 3. 500k symbol indexing peak RSS in SQLite (< 80 MiB) ---"

cargo run --release -q -p weave-graph-store-sqlite --example mem_500k 2>&1
# The example self-reports and exits non-zero if over budget
sqlite_result=$?
if [ "$sqlite_result" -eq 0 ]; then
    pass "SQLite 500k indexing within 80 MiB envelope"
else
    fail "SQLite 500k indexing exceeded 80 MiB envelope"
fi

# ---------------------------------------------------------------------------
# 4. 500k symbol indexing peak RSS in Turso (< 80 MiB)
# ---------------------------------------------------------------------------
echo ""
echo "--- 4. 500k symbol indexing peak RSS in Turso (< 80 MiB) ---"

# Build and run a Turso 500k memory benchmark if the crate exists
if [ -f "crates/weave-graph-store-turso/Cargo.toml" ]; then
    cargo run --release -q -p weave-graph-store-turso --example mem_500k 2>&1
    turso_result=$?
    if [ "$turso_result" -eq 0 ]; then
        pass "Turso tests passed"
    else
        fail "Turso tests failed"
    fi
else
    echo "    Turso crate not present; skipping"
fi

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo "============================================"
echo "  Results: ${PASS} passed, ${FAIL} failed"
echo "============================================"

if [ "$FAIL" -gt 0 ]; then
    exit 1
fi