#!/usr/bin/env bash
set -euo pipefail

# Repeats the direct release binary and reports wall-clock median/min/max.
# BENCH_MODE may be cold, warm, or both. Set BENCH_CPU to pin runs with taskset.
# Usage: BENCH_REPETITIONS=9 BENCH_MODE=both BENCH_CPU=2 bash scripts/benchmark-html-pipeline.sh [TestData directory]

workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$workspace_root"
cargo build --release -p html-renderer --bin pipeline_profile

binary="${CARGO_TARGET_DIR:-$workspace_root/../target}/release/pipeline_profile"
[[ -x "$binary" ]] || binary="$workspace_root/target/release/pipeline_profile"
repetitions="${BENCH_REPETITIONS:-9}"
mode="${BENCH_MODE:-both}"
workload_args=("$@")
samples="$(mktemp)"
trap 'rm -f "$samples"' EXIT

for ((run = 1; run <= repetitions; run++)); do
    started="$(date +%s%N)"
    if [[ -n "${BENCH_CPU:-}" ]]; then
        taskset -c "$BENCH_CPU" "$binary" --mode "$mode" "${workload_args[@]}" >/dev/null
    else
        "$binary" --mode "$mode" "${workload_args[@]}" >/dev/null
    fi
    finished="$(date +%s%N)"
    awk -v start="$started" -v finish="$finished" 'BEGIN { printf "%.3f\n", (finish - start) / 1000000 }' >>"$samples"
done

sort -n "$samples" | awk -v count="$repetitions" '
    { values[NR] = $1 }
    END {
        median = (count % 2) ? values[(count + 1) / 2] : (values[count / 2] + values[count / 2 + 1]) / 2
        printf "html-pipeline %s: n=%d  median=%.3f ms  min=%.3f ms  max=%.3f ms\n", "'"$mode"'", count, median, values[1], values[count]
    }'
