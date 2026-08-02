# Pipeline PMC profiling

Run the pipeline profiler with hardware performance counters:

```bash
cd HtmlRenderer
bash scripts/profile-html-pipeline-pmc.sh
```

The program reports pipeline stage timing on standard output. `perf stat`
reports aggregate PMCs for the same run on standard error.

Default counters:

| Counter | Diagnostic |
| --- | --- |
| `cycles`, `instructions` | IPC (`instructions / cycles`); low IPC can indicate stalls, dependency chains, or front-end pressure. |
| `branches`, `branch-misses` | Branch miss rate; a high rate points to unpredictable selector/cascade/layout decisions. |
| `l1-dcache-loads`, `l1-dcache-load-misses` | Direct L1 data-cache load miss rate. |
| `l1-icache-loads`, `l1-icache-load-misses` | Direct L1 instruction-cache load miss rate. |
| `cache-references`, `cache-misses` | Generic cache miss rate; use it as a coarse locality signal. |

The wrapper splits core, L1D, and L1I/generic-cache events into separate
passes so they do not multiplex on CPUs with few programmable PMCs. The runs
are comparable because each pass executes the same deterministic profiler.
Set `PMC_EVENTS` to run one custom raw-event pass instead.

The wrapper separately asks perf for `l1d_miss_rate` and `llc_miss_rate`.
Some kernels expose `llc_miss_rate` as a metric name but lack the PMU events
needed to compute it; perf reports `NaN` in that case. Disable this extra
single-run metric pass with `PMC_METRICS=`.

For CPUs that expose additional events through `perf list`, override the raw
event set:

```bash
PMC_EVENTS='cycles,instructions,branches,branch-misses,L1-dcache-loads,L1-dcache-load-misses,LLC-loads,LLC-load-misses' \
  bash scripts/profile-html-pipeline-pmc.sh
```

Cache and branch counters cover the entire pipeline run. Correlate them with
the pipeline stage timings to find the expensive stage, then use a focused
profile for attribution:

```bash
perf record -e cycles:u -g -- target/release/pipeline_profile TestData
perf report
```

Do not compare PMC totals between CPUs or kernel configurations. Compare runs
on the same machine, with the same release binary, TestData corpus, and event
set. The wrapper repeats the measurement three times by default; `perf stat`
reports the variation.
