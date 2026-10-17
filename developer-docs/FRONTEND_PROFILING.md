# Frontend performance profiling

Measure the release application with Chrome's existing tools before changing rendering. FPS measures delivery rate, not idle CPU or GPU cost. Keep the Rust backend, Chrome renderer, and Chrome GPU process separate.

## DevTools collection

Build production assets with source maps and a release backend:

```sh
npm --prefix web ci
npm --prefix web run build -- --sourcemap
cargo build --release --locked
./scripts/dev_run.sh --release --listen 127.0.0.1:9110
```

Open `/space` in Chrome with hardware acceleration enabled. In `chrome://gpu`, record the actual GL renderer and WebGL2 status. SwiftShader results are software-rendering results and must be labeled accordingly. Use one inspected tab, a 1440 × 1100 CSS-pixel viewport, DPR 1, no CPU/network throttling, and the same camera/search/selection for comparisons. Let loading, shader compilation, and initial camera motion settle.

In **DevTools → Performance**, record 15–30 seconds for each case:

1. Visible idle `/space`, without moving the pointer over the scene.
2. SPACE orbit, right-drag pan, and alternating zoom in/out.
3. SPACE with process/mapping updates and IPC activity. Record the workload, process/map/relation counts, update rates, and any sensor failures.
4. Visible idle `/list`. Add a process-detail recording if its behavior is being changed.

Save each trace. Inspect the flame chart and Bottom-up Self Time. Separate per-frame work from EventSource snapshot/activity callbacks; check `animate`, `drawLabels`, `buildScene`, `SpaceDataStore.replaceSnapshot`, and layout functions by their source locations. Inspect long tasks (>50 ms), GC, layout, and paint. Do not add overlapping inclusive times together. Compare CPU usage and memory in **Chrome Task Manager (Shift+Esc)** or **DevTools → Performance monitor** over the same interval. Record CPU model, GPU/driver, Chrome version, commit, viewport/DPR, render scale, visibility, and scene size beside the artifacts.

If repeated recordings show increasing retained heap, use **DevTools → Memory** allocation sampling or heap snapshots, compare after GC, and identify retaining paths. A larger heap at one endpoint does not establish a leak. If GPU work is in question, capture draw calls/resources with Spector.js or use browser GPU tools and disjoint timer queries where supported. Neither CPU time in `renderer.render()` nor the GPU process's CPU percentage measures GPU execution time. Use Linux perf/V8 JIT symbols only when the browser profile points to native overhead that needs further attribution.

## Repeatable CDP collection

The collector uses the shared [browser CDP harness](../web/tests/browser/harness.ts), `Profiler.enable/start/stop`, standard timeline tracing, `Performance.getMetrics`, and `SystemInfo.getProcessInfo`. It adds no application profiling hooks. The browser is isolated and headless, its page stays visible (`document.hidden === false`), and hardware ANGLE/EGL is requested by default. Verify the recorded GPU identity; requesting hardware does not prove it was selected. Headless results do not describe the desktop compositor or battery consumption.

With the release server above running, collect each scenario sequentially:

```sh
for scenario in idle-space interact-space busy-space idle-list; do
  npm --prefix web run profile:frontend -- \
    --url http://127.0.0.1:9110 --fixture --seconds 15 \
    --scenario "$scenario" --output ../target/frontend-profile/before
done
```

Repeat with the comparison build and `--output ../target/frontend-profile/after`. Avoid concurrent builds, tests, or other profiling browsers while comparing. Run multiple pairs for a small effect or noisy results. `CHROME` selects a different Chrome executable. `--swiftshader` explicitly selects software rendering. Omitting `--fixture` measures the live backend; generate a documented workload yourself for `busy-space` in that mode.

The fixed fixture proxies the release assets and configuration and serves synthetic API data: 256 processes, 16 mappings each (4096 total), 255 pipe relations, one structural snapshot per second, and activity messages every 100 ms. Idle snapshots repeat the same topology. Busy snapshots change mapping permissions every second, and each activity message carries 64 CPU samples and 64 IPC records (8 operations/4096 bytes each); one signal is sent per second. It does not reproduce real backend collection costs, process churn, missing-event recovery, or arbitrary topology distributions. The interaction scenario alternates orbit and pan with wheel zoom through CDP input events, paced at roughly 16 ms; browser scheduling controls the actual event rate.

Each scenario writes:

- `.cpuprofile.gz`: standard V8 CPU samples, importable into DevTools Performance after `gunzip -k`.
- `.trace.json.gz`: standard Chrome timeline, importable after decompression.
- `.json`: environment, GPU identity, raw metrics/process CPU counters, heap endpoints, counts, top sampled self times with source-map locations, frame callback durations, long tasks, GC, layout, paint, and browser errors.
- `*.js.map.gz`: available source maps for the profiled bundles. Source lines in the machine-readable summary are zero-based. Keep these maps with profiles from that build.

`processCpuPercentOfOneCore.renderer` sums CPU time deltas of surviving renderer processes in this isolated Chrome instance; 100% is one logical core. `GPU` is the GPU **process's CPU** usage. Main-thread task busy percentage is `TaskDuration / elapsed time`; it is a different metric. `FireAnimationFrame` durations describe main-thread callbacks, not frame presentation or GPU execution. Trace durations can be inspected directly in DevTools. Sampling/tracing overhead is present in both sides of a comparison. Raw traces and profiles can contain page content and local paths; review them before sharing.

The checked-in [baseline and comparison report](performance/230/REPORT.md) includes profiles and collection conditions for issue #230.
