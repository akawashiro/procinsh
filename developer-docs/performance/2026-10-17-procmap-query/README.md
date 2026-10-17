# SPACE backend CPU: text maps vs PROCMAP_QUERY

This experiment compares the same backend and Chrome workload with structural
memory mappings read as text or through Linux's binary `PROCMAP_QUERY` ioctl.
The measurements target **only the procinsh backend PID and all its threads**.
One logical CPU is 100%; Chrome's CPU is excluded from those totals.

**Outcome:** debug CPU averages are 7.75% lower, but release CPU averages are
2.44% higher. These eight intervals do **not** establish a release performance
benefit from replacing bulk text reads with per-VMA ioctls. The implementation
is an experimental candidate, not a recommendation to switch the default.

## Results

The profile-level averages aggregate CPU seconds over the two actual elapsed
measurement intervals. A negative relative change means lower backend CPU.

| Profile | Before CPU | After CPU | Relative CPU change |
|---|---:|---:|---:|
| debug | 65.36% | 60.29% | -7.75% |
| release | 19.47% | 19.94% | +2.44% |

User/kernel breakdown (percent of one CPU):

| Profile / variant | User | Kernel |
|---|---:|---:|
| debug / before | 53.86% | 11.49% |
| debug / after | 46.98% | 13.31% |
| release / before | 7.83% | 11.63% |
| release / after | 7.42% | 12.52% |

Individual 60-second intervals, in execution order:

| Profile | Order / variant | CPU | Maps at start | Maps at end |
|---|---|---:|---:|---:|
| debug | 1 / before | 60.28% | 27,838 | 29,177 |
| debug | 2 / after | 63.16% | 32,500 | 33,714 |
| debug | 3 / after | 57.41% | 33,436 | 33,902 |
| debug | 4 / before | 70.43% | 35,005 | 35,485 |
| release | 1 / before | 19.63% | 35,961 | 36,010 |
| release | 2 / after | 19.65% | 36,625 | 37,234 |
| release | 3 / after | 20.23% | 35,620 | 37,263 |
| release | 4 / before | 19.30% | 38,769 | 38,724 |

The workload changes its VMA count over time; the table exposes that difference
rather than normalizing CPU by an assumed fixed mapping count. The two-run
comparison is descriptive and cannot isolate the implementation effect from
all workload variation.

All eight intervals have 20 titled workload tabs and no logged navigation errors,
Chrome error pages, observer errors, SSE gaps or mapping-read errors. Two pages
show anti-bot challenge titles throughout. All four sensor states are
`observing`; the namespace attribution restriction below still applies.

In release, userspace CPU falls from 7.83% to 7.42%, while kernel CPU rises from
11.63% to 12.52%. The implementation makes one ioctl per VMA instead of reading
many text entries per read. The observed breakdown is consistent with syscall
and kernel traversal costs offsetting parsing savings, but this experiment does
not attribute the additional kernel CPU to a specific operation. Optimizing the
text parser or reducing collection frequency remain separate, unmeasured ideas.

## Measured implementations

- **Before:** main at
  [`7bb1b8a13cafbb0d6ca31b549dd507fdd8ed9036`](https://github.com/akawashiro/procinsh/commit/7bb1b8a13cafbb0d6ca31b549dd507fdd8ed9036).
  `read_maps` reads `/proc/<pid>/maps` as text and parses its lines.
- **After:**
  [`f90a6413230d78c28e0a7d1f809cc1578d7d5c3c`](https://github.com/akawashiro/procinsh/commit/f90a6413230d78c28e0a7d1f809cc1578d7d5c3c).
  `read_maps` enumerates VMAs using `PROCMAP_QUERY_COVERING_OR_NEXT_VMA` and
  translates the returned binary fields directly into `MemoryMap` values.

Both debug and release executables are built using Rust 1.98.1, the same lockfile
and frontend assets, in the same Ubuntu 24.04 Docker build environment.
[binary-manifest.json](binary-manifest.json) records each executable's size and
SHA-256. The application uses its normal Cargo profiles, with no custom
optimization flags. The backend starts through a copy of `scripts/dev_run.sh`.

The [kernel's binary API](https://cdn.kernel.org/doc/html/latest/filesystems/proc.html)
is available since Linux 6.11. Unsupported ioctls (`ENOTTY`) fall back to text
using the same open file descriptor; other errors propagate. Names retain the
text reader's newline escaping and trailing-whitespace handling. Name buffers
grow on `E2BIG` or `ENAMETOOLONG`, and interrupted calls are retried.

`PROCMAP_QUERY` does not return x86's `[vsyscall]` pseudo-mapping, which lies
outside the ordinary VMA tree. The binary structural view therefore omits that
entry. Process-detail RSS/PSS collection still uses `smaps`. The stable-process
comparison test checks every ordinary VMA against the text representation;
the omission is not treated as a matching map. Each queried VMA is consistent,
but a multi-ioctl traversal is not an atomic address-space snapshot.

## Measurement method

All backend processes, the workload Chrome, a separate observer Chrome, the
Node.js controller and the Python CPU reader execute inside one private-PID,
bridge-network Docker container. The runtime is Ubuntu 26.04 with Chrome
141.0.7390.65 and Node.js 22.18.0 on Linux 7.0.0-38-generic, running on an AMD
Ryzen 9 5950X with 32 logical CPUs. The local runtime image ID is
`sha256:996e1547d4f4933ffd23d6704320c470fdf59875bd44a398db2c90123422608f`.
It identifies a local image rather than a published reproducible image.

- The existing `scripts/chrome_load.mjs` opens the first 20 URLs from
  `scripts/chrome_sites.txt`, disables cache and reloads every 10 seconds.
- The same workload and observer browsers remain alive across all runs.
  The observer displays `/space` at 1280×900 with SwiftShader.
- Each run starts a fresh backend. After SPACE SSE becomes ready, there are
  **30 seconds of warmup and 60 seconds of CPU measurement**.
- For each build profile, execution order is **before, after, after, before**.
  This gives two measurements per variant and balances a simple time trend.
- CPU percentages use `/proc/<pid>/stat` user/system CPU deltas divided by the
  actual monotonic interval. Thread counters are read once per second; their
  totals can miss threads exiting between reads.
- CPU measurement runs without an external perf recorder. Builds, test suites
  and the short harness dry run finish before the reported comparison starts.
- The controller verifies 20 workload tabs, continued snapshot/activity SSE,
  nonempty structural maps, observing sensor states and no observer errors.
  It retains screenshots and records map errors, navigation errors and
  anti-bot challenge-page counts alongside each result.

The container drops all capabilities, then adds `PERFMON`, `SYS_PTRACE`, `BPF`,
`DAC_READ_SEARCH`, `SYSLOG`, `IPC_LOCK`, `SETUID`, `SETGID`, `CHOWN` and `KILL`.
The last four let the root controller start and clean up UID/GID 1000 browsers.
It uses `seccomp=unconfined`, a read-only root filesystem, a writable experiment
mount, 1 GiB `/tmp`, 256 MiB shared memory, an 8 GiB memory limit, a 2,048 PID
limit and unlimited memlock. It uses neither privileged mode nor host PID/network
namespaces. Public DNS is explicitly configured for the workload.

## Interpretation limits

This is a controlled comparison within one container, with two runs per variant;
it does not establish statistical significance or performance on other hosts.
Website content, anti-bot pages and Chrome process churn can vary even with
the same browser kept alive. The balanced order reduces but does not eliminate
those effects.

SPACE sees the container's process inventory. Its BPF sensors emit host TGIDs,
which generally do not match private-namespace PIDs. Activity attribution is
therefore incomplete, despite the sensors reporting `observing`. PID-scoped
CPU accounting also excludes BPF work performed in other tasks or interrupt
contexts. These numbers do not estimate total system monitoring overhead on
a host with a different process inventory.

The baseline is fresh main, rather than the debug build of PR #221 used in the
earlier report. The earlier report also has an external perf recorder attached.
Its absolute CPU values should not be used as this experiment's baseline.

## Evidence and repetition

- [measurements.json](measurements.json): all eight CPU and health summaries.
- [summary.json](summary.json): aggregated CPU results and per-run values.
- [health.json](health.json): workload page counts and capture errors.
- [summarize.py](summarize.py): validates all intervals and regenerates `summary.json`.
- [measure.mjs](measure.mjs) is the exact experiment controller.
- [cpu.py](cpu.py) reads backend and per-thread CPU counters.
- [binary-manifest.json](binary-manifest.json) identifies the four executables.

The controller runs as root in the experiment container:

```sh
node /experiment/measure.mjs /experiment
```

To validate the included data and reproduce the summary:

```sh
python3 developer-docs/performance/2026-10-17-procmap-query/summarize.py
```

Its input directory contains `before-source/scripts/chrome_load.mjs` and its
site list, `cpu.py`, and four runtime roots named
`runtime/{before,after}-{debug,release}/`. Each runtime root contains the
corresponding executable at `target/debug/procinsh` and the repository's
`scripts/dev_run.sh` wrapper. Release executables also use that path because the
wrapper selects it; their compiler profile is still release.

`WARMUP_SECONDS`, `SAMPLE_SECONDS`, `PROFILES` and `ORDER` override the defaults.
The results directory includes full per-second CPU reads, screenshots, backend
logs and a summarized `results.json`. Input binaries and raw captures remain in
the local experiment directory `procinsh-procmap-ab` rather than in Git.

## Validation

All suites use `bash scripts/docker_test.sh --network host <suite>` with the
implementation source copied into an independent checkout. The independent
checkout avoids a worktree `.git` reference pointing outside the container.
Host networking is used for the test suites because the default bridge DNS
cannot resolve the package repositories; the CPU experiment uses private bridge
networking with working DNS.

- `checks`: formatting, Clippy, rustdoc links, frontend API/dependency docs,
  package contents and the Node-free packaged build pass.
- `ordinary`: 102 Rust unit tests, 4 build-Git integration tests, 1 HTTP
  integration test, 107 frontend tests, listen policy and Vite integration pass;
  Rust tests have no ignored or skipped cases.
- `kernel`: scheduler, IPC, regular-file and signal live-sensor checks pass.
- `browser`: process sessions/details and SPACE's WebGL, file/network activity,
  mobile behavior and SSE lifecycle checks pass.
