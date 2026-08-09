#!/usr/bin/env bash
set -euo pipefail

# Runs a real allocation profiler over the release pipeline workload.
# PROFILE_MODE may be cold, warm, or both; default is cold.
# Usage: PROFILE_MODE=both bash scripts/profile-html-pipeline-memory.sh [TestData directory]

workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$workspace_root"
cargo build --release -p html-renderer --bin pipeline_profile

binary="${CARGO_TARGET_DIR:-$workspace_root/../target}/release/pipeline_profile"
[[ -x "$binary" ]] || binary="$workspace_root/target/release/pipeline_profile"
mode="${PROFILE_MODE:-cold}"
output_dir="${PROFILE_OUTPUT_DIR:-$workspace_root/target/pipeline-memory-profile}"
mkdir -p "$output_dir"

if command -v heaptrack >/dev/null; then
    output="$output_dir/heaptrack.$(date +%Y%m%d-%H%M%S).gz"
    heaptrack --output "$output" -- "$binary" --mode "$mode" "$@"
    echo "Heaptrack output: $output"
elif command -v valgrind >/dev/null; then
    output="$output_dir/massif.$(date +%Y%m%d-%H%M%S).out"
    valgrind --tool=massif --massif-out-file="$output" "$binary" --mode "$mode" "$@"
    echo "Massif output: $output"
else
    echo "No allocation profiler found. Install heaptrack (preferred) or valgrind, then rerun this script." >&2
    exit 2
fi
