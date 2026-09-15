#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

forbidden='^(candle|fastembed|hf-hub|hyper|libsql|onnxruntime|ort|reqwest|tokio|ureq) v'
tree=$(mktemp)
trap 'rm -f "$tree"' EXIT
cargo tree -p weave-graph-cli --no-default-features -e normal --prefix none > "$tree"

if rg -n "$forbidden" "$tree"; then
    echo "core dependency closure contains a forbidden network, model, or inference dependency" >&2
    exit 1
fi

echo "PASS: core dependency closure contains no network, model, or inference runtime"
