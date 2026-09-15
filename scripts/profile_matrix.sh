#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

output=${1:-target/profile-matrix.txt}
mkdir -p "$(dirname "$output")"

hash_file() {
    if command -v sha256sum >/dev/null; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

record_profile() {
    local name=$1
    local features=$2
    shift 2
    cargo build --release -q -p weave-graph-cli --no-default-features "$@"
    local binary=target/release/weave
    local closure
    closure=$(cargo tree -p weave-graph-cli --no-default-features -e normal --prefix none "$@" |
        awk '/ v[0-9]/{count += 1} END {print count + 0}')
    printf 'profile=%s features=%s executable_bytes=%s executable_sha256=%s dependency_entries=%s model_artifacts=not-installed\n' \
        "$name" "$features" "$(wc -c < "$binary" | tr -d ' ')" "$(hash_file "$binary")" "$closure" \
        >> "$output"
}

: > "$output"
printf 'platform=%s rustc=%s\n' "$(uname -srm)" "$(rustc --version)" >> "$output"
record_profile core none
record_profile basic fts --features fts
record_profile extended lang-extended --features lang-extended
record_profile vector vector --features vector
record_profile slm slm --features slm
cat "$output"
