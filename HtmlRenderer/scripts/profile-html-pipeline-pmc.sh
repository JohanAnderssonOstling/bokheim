#!/usr/bin/env bash
set -euo pipefail

# Profiles the full html-pipeline TestData run with Linux hardware performance
# counters. The pipeline profiler writes its stage timings to stdout while perf
# writes aggregate PMCs to stderr, so both can be captured independently.
#
# Usage:
#   bash scripts/profile-html-pipeline-pmc.sh [TestData directory]
#
# Override PMC_EVENTS for architecture-specific counters, for example:
#   PMC_EVENTS='cycles,instructions,branches,branch-misses,L1-dcache-loads,L1-dcache-load-misses,LLC-loads,LLC-load-misses' \
#     bash scripts/profile-html-pipeline-pmc.sh

workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$workspace_root"
workload_args=("$@")

cargo build --release -p html-renderer --bin pipeline_profile

repetitions="${PMC_REPEAT:-3}"
metrics="${PMC_METRICS:-l1d_miss_rate,llc_miss_rate}"
binary="${CARGO_TARGET_DIR:-$workspace_root/../target}/release/pipeline_profile"
if [[ ! -x "$binary" ]]; then
    binary="$workspace_root/target/release/pipeline_profile"
fi

echo "Interpretation: instructions/cycles is IPC; branch-misses/branches is the branch miss rate; L1 cache misses divided by loads is the L1 miss rate; cache-misses/cache-references is the generic cache miss rate." >&2
echo "Derived metrics: ${metrics:-disabled}. A NaN LLC metric means the kernel does not expose the required LLC PMU events." >&2

run_pmc_pass() {
    local label="$1"
    local events="$2"
    echo >&2
    echo "=== $label: $events ===" >&2
    perf stat --repeat "$repetitions" --event "$events" -- "$binary" "${workload_args[@]}"
}

# Keep every default pass at four programmable events or fewer. Combining them
# causes multiplexing on common CPUs and makes miss rates less trustworthy.
if [[ -n "${PMC_EVENTS:-}" ]]; then
    run_pmc_pass "custom PMC events" "$PMC_EVENTS"
else
    run_pmc_pass "core execution" "cycles,instructions,branches,branch-misses"
    run_pmc_pass "L1 data cache" "l1-dcache-loads,l1-dcache-load-misses"
    run_pmc_pass "L1 instruction and generic cache" "l1-icache-loads,l1-icache-load-misses,cache-references,cache-misses"
fi

if [[ -n "$metrics" ]]; then
    echo >&2
    echo "Derived cache metrics (single run):" >&2
    perf stat --metric-only --metrics "$metrics" -- "$binary" "${workload_args[@]}"
fi
