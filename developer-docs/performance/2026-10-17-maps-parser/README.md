# SPACE: release perf and text-map parser experiments

The text-map parser can process this captured input **1.70× faster** by using
five stack-allocated column slices and ASCII separator scanning. A SIMD-capable
`memchr` candidate reaches 1.63× and does not beat the simpler byte scanner on
this machine. These are parser replay results, not whole-backend CPU reductions.

## What release perf shows

The baseline is main
[`7bb1b8a`](https://github.com/akawashiro/procinsh/commit/7bb1b8a13cafbb0d6ca31b549dd507fdd8ed9036),
built with the ordinary Cargo release profile and Rust 1.98.1. With 20 Chrome
tabs reloading every 10 seconds and a separate Chrome displaying SPACE, the
backend PID is sampled at 199 Hz for 60 seconds after 30 seconds of warmup.

There are **1,738 samples**, with zero lost samples reported by perf:

| Executing code | Share of samples |
|---|---:|
| Kernel symbols | 63.69% |
| procinsh, including statically linked Rust dependencies | 17.61% |
| libc | 16.28% |
| Unknown DSO | 1.38% |
| BPF trampolines / overlay | 1.04% |

Selected **self/leaf** samples, as percentages of all backend samples:

| Symbol | Share | Relevant work |
|---|---:|---|
| `established_get_first` | 3.39% | TCP connection-table traversal |
| `mangle_path` | 2.65% | Kernel path-name formatting |
| `memcpy_orig` | 2.59% | Kernel copying; callers are not attributed here |
| `htab_map_update_elem` | 2.42% | BPF hash-map updates in the sampled task |
| `serde_json::ser::format_escaped_str_contents` | 2.07% | JSON string encoding |
| `lock_next_vma` | 1.78% | Kernel VMA traversal |
| `maps::parse_map` | 1.73% | Map metadata parsing |
| `prepend_path` | 1.32% | Kernel path construction |
| `seq_put_hex_ll` | 1.27% | Kernel text formatting |
| `statx` in libc | 1.21% | Metadata syscall entry |
| `show_map_vma` | 1.04% | Text mapping output |
| `do_task_stat` | 0.98% | Process stat output |

This is a leaf-only profile, not an inclusive callgraph. Parsing also incurs
helper and allocator work elsewhere, so 1.73% is not the full parsing cost.
Kernel work dominates this release recording; the earlier debug-build inclusive
parsing percentages do not describe this release build.

A separate 30-second recording of the optimized backend, taken outside its CPU
comparison interval, contains 898 samples: 65.59% kernel, 16.37% procinsh and
15.48% libc. Kernel work remains the largest category. The recordings differ in
duration and live workload state, and leaf helper/inlining boundaries can differ;
their per-function percentages are not a controlled speedup measurement. Some
private libc functions remain unresolved within the known libc DSO.

Other concrete investigation targets in the measured source are:

- [`system::snapshot::collect`](../../../src/http_server/system/snapshot.rs)
  reads TCP/UDP/UNIX tables once per discovered network namespace per scan and
  examines process file descriptors. The TCP/path/stat symbols above are
  consistent with this collection work. Changing the parser cannot remove that
  kernel traversal and text-generation cost.
- [`SnapshotEncoder::encode`](../../../src/http_server/system/delivery.rs)
  serializes both changed map deltas and replacement maps into temporary byte
  vectors to choose the smaller representation, then serializes the selected
  payload. The JSON-encoding samples make this a candidate for avoiding
  redundant serialization; they do not quantify its complete inclusive cost.
  The snapshot-encoding proposal in [PR #221](https://github.com/akawashiro/procinsh/pull/221)
  is separate from this measured branch and is not evaluated by these numbers.
- [`SocketMetadataReader::read`](../../../src/http_server/process/socket_metadata.rs)
  validates process identity around socket duplication and publication. Any
  attempt to reduce repeated stat reads must preserve PID/FD reuse checks.

These are profiling leads rather than independently measured speedups. This
change only optimizes the map parser, allowing its effect to be measured alone.

## Replay benchmark: allocation, ASCII and SIMD

[maps-fixture.maps.gz](maps-fixture.maps.gz) contains **28,449 real mapping
lines** captured from the experiment container. Four parsers produce identical
mapping values on every line before timing. Each timed measurement parses and
drops the complete buffer 200 times; five rounds rotate the variant order.
The metric uses process CPU time, includes output/pathname allocation and drop,
and excludes procfs reads and kernel text formatting.

| Parser | Median million rows / CPU second | Throughput vs baseline |
|---|---:|---:|
| Unicode separators, temporary `Vec` | 4.725 | 1.00× |
| Unicode separators, `[&str; 5]` | 6.208 | 1.31× |
| ASCII separators, `[&str; 5]` | 8.054 | 1.70× |
| SIMD-capable `memchr3`, `[&str; 5]` | 7.690 | 1.63× |

AVX2 is available on the AMD Ryzen 9 5950X used here. The experimental SIMD
candidate uses `memchr` 2.8.3 with its default `std` feature, which enables
[runtime AVX2 dispatch](https://docs.rs/memchr/2.8.3/memchr/). Its per-search
implementation can still select a short-input path. These results do not prove
that every custom AVX2 parser would lose, but they provide no reason to add an
architecture-specific implementation for these short fields.

The selected implementation is
[`4bc17e3`](https://github.com/akawashiro/procinsh/commit/4bc17e31a86aad27cc4e567abc1ae4d70e0d0fe2).
It replaces the per-line metadata `Vec` with a five-element array and scans
ASCII separators. Numeric conversions are unchanged. Kernel-generated metadata
uses ASCII separators; UTF-8 and embedded spaces in pathnames remain intact.
Initial/trailing line trimming retains the existing behavior. Malformed and
overflowing fields still return errors. No dependency or CPU-feature requirement
is added to procinsh.

## Whole-backend CPU comparison

The average aggregates CPU seconds over both actual elapsed intervals. One
logical CPU is 100%. These four intervals have no external perf recorder attached.

| Variant | Average CPU | User CPU | Kernel CPU |
|---|---:|---:|---:|
| before | 15.00% | 5.49% | 9.51% |
| after | 14.58% | 5.02% | 9.56% |

The observed relative change is **-2.78%**.
Each variant has only two measurements; their ranges overlap, so this small
difference should not be treated as a statistically established whole-backend
speedup. The fixed-input replay isolates the parser improvement more clearly.

| Order / variant | CPU | Maps at start | Maps at end |
|---|---:|---:|---:|
| 1 / before | 14.73% | 29,965 | 31,699 |
| 2 / after | 14.25% | 33,707 | 34,965 |
| 3 / after | 14.92% | 36,555 | 36,828 |
| 4 / before | 15.27% | 36,410 | 37,199 |

All intervals retain 20 titled workload tabs, including two challenge pages,
with no logged navigation, observer or map-read errors. Snapshot/activity SSE
continues and all four sensor states are observing. The existing namespace
attribution limitation still applies.

## Environment and interpretation

The backend, workload Chrome, observer Chrome, Node controller, CPU reader and
perf all run inside one private-PID, bridge-network container. The runtime image
is `sha256:996e1547d4f4933ffd23d6704320c470fdf59875bd44a398db2c90123422608f`,
with Chrome 141.0.7390.65, Node 22.18.0 and Linux 7.0.0-38-generic.
The before/after application builds use the same Ubuntu 24.04 build image,
lockfile, frontend sources and normal release profile. Binary hashes are in
[binary-manifest.json](binary-manifest.json).

The container uses a read-only root, a writable experiment mount, 1 GiB `/tmp`,
256 MiB shared memory, an 8 GiB memory limit, 2,048 PID limit and unlimited
memlock. Capabilities and `seccomp=unconfined` enable the existing sensors and
perf; browsers run as UID/GID 1000. Host PID/network namespaces and privileged
mode are not used. The workload is the first 20 entries in
`scripts/chrome_sites.txt`, without cache, reloaded every 10 seconds.

CPU percentages refer only to the backend PID and all its threads, with one
logical CPU = 100%. They exclude Chrome/controller CPU and BPF work executing in
other tasks or interrupt contexts. BPF host TGIDs do not generally match private
namespace PIDs, so SPACE activity attribution is incomplete. The inventory is
the container's processes. Website content, challenges and process/VMA counts
vary; a small number of runs does not establish statistical significance.

## Artifacts and repetition

- [measurements.json](measurements.json) and [cpu-summary.json](cpu-summary.json):
  all four backend CPU intervals and their aggregate.
- [summarize.py](summarize.py): validates the CPU calculations and regenerates
  both CPU and parser summaries.
- [parser-summary.json](parser-summary.json) and
  [parser-bench.jsonl](parser-bench.jsonl): all replay timings and their summary.
- [parser-bench/](parser-bench/): standalone replay source and pinned lockfile.
- [input-sha256.json](input-sha256.json): uncompressed input checksum.
- [perf-before.json](perf-before.json) and
  [perf-before-leaves.txt.gz](perf-before-leaves.txt.gz): complete baseline
  self-sample counts and their symbolized leaf-only input.
- [perf-after.json](perf-after.json) and
  [perf-after-leaves.txt.gz](perf-after-leaves.txt.gz): optimized self-sample
  counts and symbolized input. Published sample streams retain all symbols and
  DSOs, with instruction addresses, PIDs and timestamps normalized.
- [perf-provenance.json](perf-provenance.json): target PIDs, recording parameters
  and original leaf-text checksums; all published samples are checked against
  their target PID before normalization.
- [analyze_perf.py](analyze_perf.py): regenerates the self-sample summary without
  inventing caller attribution.
- [measure.mjs](measure.mjs) and [cpu.py](cpu.py): the CPU comparison controller
  and backend/per-thread CPU counter reader.

Replay compilation and execution take place in Docker with Rust 1.98.1:

```sh
gzip -dc maps-fixture.maps.gz > /tmp/maps-fixture.maps
cargo +1.98.1 run --release --locked --manifest-path parser-bench/Cargo.toml \
  -- /tmp/maps-fixture.maps 200
```

The controller expects an experiment root containing the workload script/site
list, CPU reader and `runtime/{before,after}-release` directories. Each runtime
directory supplies the executable at `target/debug/procinsh` and the repository's
`scripts/dev_run.sh`; that path is selected by the wrapper, while the executable
itself is still built in release mode. `PROFILES=release` selects this comparison.
The default order is before/after/after/before, with 30 seconds of warmup and
60 seconds of measurement. Diagnostic perf recordings run outside the CPU
comparison intervals. Raw perf recordings, ELF binaries and screenshots remain
local under experiment `procinsh-maps-parser-profile`.

## Validation

All four suites run through `bash scripts/docker_test.sh --network host <suite>`
from an independent checkout containing the implementation. This avoids
worktree Git pointers outside the container, and host networking works around
the build environment's bridge DNS failure. The actual experiment uses bridge
networking with explicit DNS.

- `checks`: format, Clippy, rustdoc, frontend docs and crate packaging pass.
- `ordinary`: 101 Rust unit tests, 4 build-Git tests, 1 HTTP test, 107 frontend
  tests, listen policy and Vite integration pass; no Rust cases ignored/skipped.
- `kernel`: scheduler, IPC, regular-file and signal checks pass.
- `browser`: process sessions/details and SPACE WebGL, mobile and SSE checks pass.
- The replay checks equality of all four candidate outputs over every fixture
  row. Product tests cover UTF-8/spaced paths, optional names, permissions,
  boundaries, invalid columns and numeric overflow.
