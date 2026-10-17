# Frontend function-time baseline — issue #230

The baseline identifies which functions consume time in the unoptimized frontend. Start with the function tables below; [full rankings](FUNCTION_TIMES.md) include the top 15 self and inclusive times in each scenario, and [FUNCTION_TIMES.csv](FUNCTION_TIMES.csv) contains every sampled function.

## What the times mean

**All function times are milliseconds accumulated over an approximately 15-second recording.** They are sampling estimates, not milliseconds per invocation. **Self** excludes callees; **total** includes callees. Parent and child totals overlap: for example, `animate` contains `drawLabels` and WebGL rendering, and `drawLabels` contains its projection helper `add` and native `measureText`. Do not add these rows together.

Percentages below divide by non-idle sampled time, including unattributed `(program)` and GC samples. Idle SPACE has 1205.6 ms of non-idle samples in a 15.040-second sampling window, so 346.9 ms in labels is 28.8% of active samples, rather than 28.8% of wall-clock time or all machine CPUs. Process CPU and main-thread task busy time are separate counters.

## Idle SPACE: where the time goes

| Function | Self (ms) | Total including callees (ms) | Total / active samples |
| --- | ---: | ---: | ---: |
| `animate` | 14.8 | 609.1 | 50.5% |
| `drawLabels` | 69.2 | 346.9 | 28.8% |
| `add` | 123.0 | 127.2 | 10.6% |
| `measureText` | 79.3 | 79.3 | 6.6% |
| `render` | 3.2 | 216.9 | 18.0% |
| `buildScene` | 24.1 | 117.4 | 9.7% |
| `replaceSnapshot` | 2.1 | 41.3 | 3.4% |
| `(program)` | 324.6 | 324.6 | 26.9% |
| `(garbage collector)` | 35.1 | 35.1 | 2.9% |

`drawLabels` is the largest named child subtree of `animate`. Its `add` helper spends about 123 ms in projection/label preparation itself, and native `measureText` contributes about 79 ms. Scene construction and snapshot replacement also appear, but cost less than label work in this capture. `(program)` remains unattributed native/program execution and is not assigned to a JavaScript function. The CPU profiler's GC estimate and the timeline's 26.1 ms of top-level GC are different observations; neither indicates dominant GC pauses here.

## SPACE scenarios: total / self milliseconds

Each cell is **total / self**, over that scenario's whole recording. Zero sampled time does not establish that a function was never called.

| Function | Visible idle | Orbit/pan/zoom | Mapping/IPC activity |
| --- | ---: | ---: | ---: |
| `animate` (space/renderer.ts) | 609.1 / 14.8 | 595.9 / 17.2 | 1535.3 / 454.3 |
| `drawLabels` (space/renderer.ts) | 346.9 / 69.2 | 366.7 / 67.8 | 312.5 / 50.8 |
| `add` (space/renderer.ts) | 127.2 / 123.0 | 146.7 / 143.5 | 116.5 / 110.2 |
| `render` (three.module.js) | 216.9 / 3.2 | 196.7 / 2.1 | 197.2 / 1.1 |
| `buildScene` (space/scene.ts) | 117.4 / 24.1 | 110.0 / 19.0 | 119.7 / 28.6 |
| `replaceSnapshot` (space/data.ts) | 41.3 / 2.1 | 41.2 / 1.1 | 45.5 / 0.0 |
| `pointermove` (space/selection.ts) | 0 sampled | 438.8 / 6.3 | 0 sampled |
| `edgeHit` (space/selection.ts) | 0 sampled | 420.5 / 1.1 | 0 sampled |
| `ingestActivity` (space/data.ts) | 14.8 / 1.1 | 12.7 / 1.1 | 213.6 / 51.8 |
| `setHex` (three.core.js) | 7.4 / 7.4 | 1.1 / 1.1 | 367.5 / 367.5 |

During interaction, the `pointermove` handler consumes 438.8 ms including callees (26.2% of active samples). Its `edgeHit` path accounts for 420.5 ms (25.1%), mainly ray intersection work; this is a substantial cost alongside `drawLabels` at 366.7 ms (21.9%). This finding comes from the input scenario and should not be generalized to stationary idle time.

During activity, `animate` grows to 1535.3 ms including callees (62.6% of active samples), with 454.3 ms of self time. Three.js color construction and `setHex` are prominent: `setHex` alone has 367.5 ms self time (15.0%). Activity ingestion contributes 213.6 ms including callees (8.7%). Labels remain about 312.5 ms (12.7%). The [busy ranking](FUNCTION_TIMES.md#busy-space) exposes both parent and child rows instead of attributing all this time to drawing.

## Idle LIST

`ListDataStore.refresh` has 118.1 ms including callees (21.1% of 558.9 ms of non-idle samples). `renderProcesses` accounts for 113.8 ms, including 56.0 ms in `Display.bytes`; native `replaceChildren` contributes 32.8 ms self time. Unattributed `(program)` samples are 427.0 ms (76.4%), so most active sampled time cannot be assigned to a named JavaScript function from this profile alone. See the [LIST ranking](FUNCTION_TIMES.md#idle-list).

## Resource and timeline context

CPU percentages use one logical core as 100%. Renderer CPU includes renderer threads; task busy percentage measures main-thread task duration. Animation callback p95 is main-thread callback time, not presentation latency or GPU execution.

| Scenario | Renderer CPU | Main-thread busy | Non-idle sampled time | Animation callback p95 |
| --- | ---: | ---: | ---: | ---: |
| idle-space | 13.67% | 8.13% | 1205.6 ms | 0.700 ms |
| interact-space | 17.39% | 11.27% | 1672.1 ms | 0.800 ms |
| busy-space | 23.07% | 16.48% | 2453.3 ms | 1.972 ms |
| idle-list | 7.13% | 3.72% | 558.9 ms | N/A |

All three SPACE captures show approximately 60 animation callbacks per second and 100% render scale. None of the four captures has a main-thread task longer than 50 ms. Function-level profiles distinguish steady frame work, input/picking work, and SSE-triggered snapshot/activity work; FPS alone does not reveal these costs.

## Collection conditions and artifacts

Collection date: 2026-10-17. The measured frontend is main at `7bb1b8a13cafbb0d6ca31b549dd507fdd8ed9036`, with production source-map assets embedded in a release backend. The SPACE bundle is `space-DzZFTMMs.js`. The collection utility is archived at `ca9ca0c1a02c01306b2b15d779265e3b86e22918`, whose frontend is identical to the measured main. The metadata fields named before/after describe the beginning/end of one baseline recording. Checkout/dirty fields describe the collector checkout; source edits made while the baseline binary was running were not embedded into it.

Hardware: AMD Ryzen 9 5950X (32 logical CPUs), NVIDIA GeForce RTX 5060 Ti, NVIDIA driver 580.178.04, Linux 7.0.0-38-generic. Browser: Chrome 141.0.7390.65, hardware ANGLE OpenGL/EGL, isolated headless browser, visible document (`document.hidden === false`), 1440 × 1100 CSS pixels, DPR 1. Each case has a fresh browser, five seconds of settling after the initial FPS indicator, and one 15-second collection without concurrent builds/tests. Chrome's standard DevTools CPU profiler/timeline and process counters were accessed through CDP; no interactive desktop DevTools UI or Task Manager was operated.

The synthetic fixture has 256 processes, 16 mappings per process (4096 total), 255 pipe relations, one snapshot per second, and activity messages every 100 ms. Idle snapshots repeat the same topology. Busy snapshots change mapping permissions; each activity message contains 64 CPU records and 64 IPC records (8 operations/4096 bytes each), with one signal per second. The input case alternates orbit/right-drag pan and zoom via CDP input at approximately 16 ms pacing. LIST displays the 256 process summaries. See [collection and reproduction](../../FRONTEND_PROFILING.md).

| Scenario | V8 profile | Environment and metrics | Chrome timeline |
| --- | --- | --- | --- |
| idle-space | [profile](baseline/idle-space.cpuprofile.gz) | [metadata](baseline/idle-space.json) | [timeline](baseline/idle-space.trace.json.gz) |
| interact-space | [profile](baseline/interact-space.cpuprofile.gz) | [metadata](baseline/interact-space.json) | Not checked in |
| busy-space | [profile](baseline/busy-space.cpuprofile.gz) | [metadata](baseline/busy-space.json) | Not checked in |
| idle-list | [profile](baseline/idle-list.cpuprofile.gz) | [metadata](baseline/idle-list.json) | Not checked in |

Decompress profiles/timelines with `gunzip -k` and import them in DevTools Performance. Source-map archives alongside the profiles retain original definitions. [SHA256SUMS](SHA256SUMS) verifies raw and derived artifacts. The other three timeline captures remain under `target/frontend-profile/before` in the collection workspace; their metrics and raw CPU profiles are checked in here.

## Aggregation and limits

Function identities use original source path and mapped definition line/column, with original names resolved from source-map names or definition headers when minified. Native/V8 frames use their recorded names. For each non-idle sample, add its `timeDeltas[i] / 1000` milliseconds to the leaf function's self time and to every distinct ancestor function's total. Recursive appearances of a function in the same stack count once. Group matching definitions across call-tree contexts, sort by self or total time, and divide by the sum of non-idle sample weights for percentages. The sum of all function self times matches non-idle sampled time; each function's self time is at most its total. The CSV retains six decimal places for numeric validation; displayed tables round to one decimal place. Definition links and ranking line numbers are one-based, while raw source-map metadata uses zero-based lines.

These are single recordings with profiling overhead and a fixed synthetic topology. Sampled costs are estimates and do not provide call counts, per-call latency, or statistical significance. GPU execution time, power, desktop presentation, retained-heap growth, and Rust backend costs are outside the measurement. The Chrome GPU process's CPU counter is not GPU utilization. The profile supports choosing where to investigate next; it does not establish the benefit of a proposed rendering, geometry, IPC, or DOM change.
