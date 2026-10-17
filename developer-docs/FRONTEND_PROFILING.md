# Frontend baseline profiling

The [issue #230 report](performance/230/REPORT.md) records the unoptimized frontend at `7bb1b8a13cafbb0d6ca31b549dd507fdd8ed9036`. It includes [function rankings](performance/230/FUNCTION_TIMES.md), [all function times as CSV](performance/230/FUNCTION_TIMES.csv), raw profiles, source maps, and collection conditions.

## Reading the results

Decompress a `.cpuprofile.gz` with `gunzip -k` and import the resulting `.cpuprofile` in Chrome DevTools Performance. Import the decompressed idle SPACE `.trace.json` to inspect the timeline, animation callbacks, SSE tasks, GC, layout, and paint. The rankings already resolve bundled source locations to original definitions.

Self time is attributed to the function executing when a sample was taken. Total time includes its descendants. A parent's total overlaps with its children's totals. Times cover the entire recording; they are neither exact timers nor per-call latencies. The active-time denominator includes native, `(program)`, and GC samples and excludes `(idle)` samples. GPU execution time is outside these CPU measurements.

## Collection and reproduction

Collection uses the standard Chrome DevTools CPU profiler and timeline through the repository's existing CDP harness: `Profiler.enable/start/stop`, `Tracing.start/end`, `Performance.getMetrics`, and `SystemInfo.getProcessInfo`. No application profiling hooks are required. Inspect flame charts and Bottom-up Self Time before selecting an optimization, and keep renderer CPU separate from the Rust backend and Chrome GPU process CPU.

The collector and synthetic fixture used for these captures are preserved at [collection commit ca9ca0c](https://github.com/akawashiro/procinsh/commit/ca9ca0c1a02c01306b2b15d779265e3b86e22918). That commit has the same frontend as the measured main revision. The commands below run in a separate checkout of that collection revision; its `profile:frontend` command and release launcher option belong to that archived checkout.

```sh
gh repo clone akawashiro/procinsh /tmp/procinsh-profile-230
cd /tmp/procinsh-profile-230
git switch --detach ca9ca0c1a02c01306b2b15d779265e3b86e22918
npm --prefix web ci
npm --prefix web run build -- --sourcemap
cargo build --release --locked
./scripts/dev_run.sh --release --listen 127.0.0.1:9110
```

In another terminal in that checkout:

```sh
for scenario in idle-space interact-space busy-space idle-list; do
  npm --prefix web run profile:frontend -- \
    --url http://127.0.0.1:9110 --fixture --seconds 15 \
    --scenario "$scenario" --output ../target/frontend-profile/baseline
done
```

Run scenarios sequentially without concurrent builds/tests. Record the actual GPU renderer, browser/driver versions, commit, viewport/DPR, visibility, counts, and workload beside each recording. The fixture's fixed topology and event rates are described in the report. Headless hardware rendering does not measure desktop presentation or battery consumption. Live backend measurements require documenting the real workload and sensor availability separately.

For an interactive investigation, use DevTools Performance for a 15–30 second recording of each scenario, inspect Flame Chart and Bottom-up Self Time, and observe CPU/memory with Task Manager or Performance monitor. Memory allocation sampling or heap snapshots are useful if repeated captures show retained growth. GPU frame/resource capture and actual GPU timer queries are needed before claiming GPU-bound behavior.
