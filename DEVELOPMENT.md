# ProcInSh Development Guide

ProcInSh is a web application for observing Linux x86-64 processes. Its Rust HTTP server collects information from `/proc`, perf, and eBPF and serves it to the browser.

This guide describes the implementation, behavior, APIs, and development and verification procedures. Run the commands below from the repository root.

## Build and runtime environment

The project uses Rust edition 2024 and rustup. `rust-toolchain.toml` pins the Rust version, rustfmt, and Clippy. Local development and CI use the same configuration; rustup installs the required toolchain automatically. To update Rust, edit this file and rerun formatting, Clippy, and tests. Building requires Node.js 22.12 or later, npm, a C compiler, `ar`, clang with a BPF backend, bpftool, pkg-config, libelf and zlib development files, and the running kernel's `/sys/kernel/btf/vmlinux`.

`build.rs` uses bpftool to generate `vmlinux.h` from BTF, then `libbpf-cargo` to build BPF objects for CPU/IPC, file I/O, and signals. Regular `cargo build` runs this step too, so these build dependencies are required even when the UI does not use BPF.

```sh
npm --prefix web ci
npm --prefix web run build
cargo build --locked
./scripts/dev_run.sh --listen 127.0.0.1:9090

# Release build
cargo build --release --locked
```

Open http://127.0.0.1:9090 in a browser. The web UI uses vanilla TypeScript in the separate `web/` project. `npm --prefix web run build` checks types, then uses Vite's multipage build to generate list, process, and space HTML, CSS, and JavaScript in `web/dist/`. It bundles shared modules and Three.js from npm (revision 180), with hashed asset names. Generated files are not tracked in Git, and Cargo does not run npm automatically. If the generated files are missing, the Cargo build fails with preparation instructions.

### Frontend development with Vite

First run `npm --prefix web ci && npm --prefix web run build && cargo build --locked`. Start the backend in one terminal and Vite in another.

```sh
# Terminal 1 (repository root)
./scripts/dev_run.sh --listen 127.0.0.1:9090

# Terminal 2
cd web
npm run dev
```

Open the URL printed by Vite (usually http://127.0.0.1:5173). Existing page URLs and process identity queries work as usual. HMR or a page reload applies frontend changes without rebuilding Rust. Vite proxies all of `/api` to the backend, allowing JSON and process/system SSE to use the same origin. Change the backend URL with `PROCINSH_BACKEND_URL=http://127.0.0.1:8080 npm run dev` or `web/.env.local`. The development header shows the version from Cargo.toml and a development indicator. In production, Rust inserts the Git SHA by replacing an HTML placeholder.

In production, `rust-embed` embeds all of `web/dist/` in the binary and serves files with MIME types based on their extensions. Debug builds also use embedded files. After changing TypeScript, HTML, or CSS, run `npm --prefix web run build` and rebuild the Rust binary. Cargo watches generated files but does not check whether they are up to date with the sources. Node.js, npm, and external CDNs are unnecessary at runtime. SPACE requires WebGL2.

Each page header shows the binary's version and, when available, a seven-character Git SHA. The SHA links separately to the GitHub commit; its accessible label contains the full SHA. `build.rs` prefers a 40-character hexadecimal SHA from `PROCINSH_GIT_SHA`. Otherwise, it reads the Git checkout containing the manifest directory and appends `-dirty` when tracked files have changes. Distribution builds without Git metadata, or builds explicitly setting `PROCINSH_GIT_SHA=""`, show only the version. HTTP requests do not read Git or the checkout.

| CLI option | Behavior |
|---|---|
| `--listen ADDRESS` | Listen address. Repeat to listen on multiple addresses. Default: `127.0.0.1:8080` |
| `--allow-non-loopback` | Explicitly allow listening on non-loopback addresses. This exposes process memory and environment variables without authentication or TLS |
| `--help` / `--version` | Show help / version |

SIGINT (Ctrl+C) or SIGTERM stops collection and shuts down the HTTP server. Fatal startup or server failures exit with a nonzero status.

### Verification before publishing to crates.io

The package includes generated `web/dist/**` through `include` in `Cargo.toml`. Integration tests and C fixtures in `tests/` are excluded; unit tests in `src/` are included as part of the source. Generated files are not tracked in Git. Before publishing, refresh them and verify that the package builds on its own:

```sh
npm --prefix web ci
npm --prefix web run build
python3 scripts/check_web_package.py
cargo publish --dry-run
```

`scripts/check_web_package.py` verifies that the crate contains all dist files, excludes `tests/`, and builds from an extracted crate without Git metadata while node/npm/npx are forced to fail. `cargo publish --dry-run` does not publish anything. `cargo install procinsh --locked` uses bundled JavaScript, so the installation machine needs no Node.js or npm. Rust and the native/BPF build dependencies are still required.

### Releases

From a checkout of this repository, run one of:

```sh
./scripts/release.sh patch  # 0.1.13 -> 0.1.14
./scripts/release.sh minor  # 0.1.13 -> 0.2.0
./scripts/release.sh major  # 0.1.13 -> 1.0.0
```

The script verifies the crates.io publication, pushes the version tag, and creates a published GitHub Release titled `v<version>` with automatically generated release notes and no uploaded assets. The notes start with a link to that version on crates.io, list pull requests merged since the previous version tag (`v<previous-version>`), and include a comparison link. If GitHub Release creation fails, the published crate, tag, and release worktree are retained for manual inspection.

## Implementation structure

| Location | Responsibility |
|---|---|
| `src/main.rs` | CLI, logger initialization, startup configuration validation |
| `src/http_server/` | Server startup/shutdown, router/middleware, worker shutdown and joining |
| `src/http_server/api/` | HTTP input/responses, domain error to status conversion, JSON and SSE (Server-Sent Events) |
| `src/http_server/web.rs` | Static web UI serving |
| `src/http_server/process/monitoring/` | Independent observations per connection, 60-second history, latest-state delivery |
| `src/http_server/process/` | `/proc` parsing, PID identity, process/thread/memory/FD/socket information |
| `src/http_server/process/snapshot/` | Registers/stacks from initial and 10-second ptrace captures and continuous perf sampling, DWARF CFI unwinding with framehop, disassembly |
| `src/http_server/process/snapshot/symbol/` | ELF retrieval/caching, address conversion, symbol/source location resolution using ELF/DWARF |
| `src/http_server/process/snapshot/stack.rs` | Frame types shared by sampling and symbol resolution |
| `src/http_server/system/` | Structure of all processes, subscriptions, BPF collection, name resolution |
| `src/http_server/system/hardware/` | GPUs, RDMA NICs, accelerators, NVSwitches, PCIe bridges, inter-device paths, and processes holding devices open, using sysfs/procfs |
| `src/http_server/netlink.rs` | Netlink dumps with deadlines and attribute parsing shared by UNIX socket diagnostics and other users |
| `web/src/` | TypeScript pages, SPACE, rendering models, and CSS (HTML entry points: `web/{list,process,space}/index.html`) |
| `tests/` | Rust, browser, log, and live sensor tests, plus C fixtures |

`main.rs` starts the unprivileged `application` module tree. The separate
`procinsh-privileged-helper` target owns `http_server/helper_root.rs` and all
inspection modules. The small library exposes only protocol definitions and
bounded transport. See [privilege separation](#privileged-helper-boundary)
for the ownership inventory, deployment, IPC limits and boundary checks. Modules live at the lowest common ancestor of their direct consumers. Child module declarations are private; items needed by parents generally use `pub(super)`.

Rust type names express responsibilities. Data and values describe their contents (`ProcessId`, `ProcessSummary`, `ProcessSnapshot`); types managing operations, state, or resources describe their roles (`ProcessScanner`, `ProcessMonitor`, `FileActivityCollector`, `PerfEventReader`, `PtraceGuard`). Helper methods alone do not determine this distinction, and types do not all require mechanical suffixes.

Hierarchical modules use `foo.rs` plus `foo/`, without `mod.rs`. `foo.rs` contains only module documentation, child module declarations, and re-exports. Implementations of types, functions, and constants live in child files named for their responsibilities.

HTTP startup/shutdown lives in `http_server/server.rs`, shared state in `state.rs`, router assembly in `router.rs` and `api/router.rs`, and HTTP guards/access logs in `middleware.rs`. The process façade is implemented in `process/identity.rs` and `process/resources.rs`, observation lifecycle in `monitoring/service.rs`, and snapshot orchestration in `snapshot/capture.rs`. Symbol handling is split into `symbol/cache.rs` and `symbol/resolve.rs`; system monitoring uses `system/service.rs` and state logging in `status.rs`. `activity.rs` owns BPF sensors and manages collection, status, and loss counts. Dependency diagrams include these child modules; overview diagrams group them by subsystem.

HTTP handlers and system monitoring use only the `process.rs` façade. Within `process`, `monitoring` handles continuous observation; `snapshot` handles stopped-state capture with ptrace initially and every 10 seconds, plus continuous perf sampling without stopping the target. Each collector's sampler owns perf FDs, mmap mappings, and an ELF cache. `process` and `system` do not depend on Axum types.

As an exception, domain types and subscription operations re-exported through façades use `pub(in crate::http_server)`. This visibility allows parents to re-export items from private children; it does not expose them outside the crate. Internal tests live in their respective modules. `tests/http.rs` starts the binary to verify HTTP, SSE, and SIGTERM shutdown.

Tokio/Axum handles HTTP and SSE; blocking detail APIs use `spawn_blocking`. Periodic process observation and system-wide structure/activity collection run on OS threads. Process detail monitoring has independent state per connection. `AppState` holds the process list scanner and subsystems. Process monitoring manages connection counts and collectors; each collector manages its perf lifecycle and symbol cache.

Web UI API types live in `web/src/shared/api-types.ts` and are maintained alongside Rust JSON responses. The contract includes null handling and hexadecimal address strings. These are compile-time types, not runtime input validation. Vite bundles Three.js as an npm dependency; TypeScript and type definitions are build-only npm dependencies.

[API Documentation](https://akawashiro.github.io/procinsh/) links to the Rust API, [TypeScript API](https://akawashiro.github.io/procinsh/typescript/), [Rust dependency graph](https://akawashiro.github.io/procinsh/rust-architecture/), and [TypeScript dependency graph](https://akawashiro.github.io/procinsh/typescript-architecture/).
Each module root's `//!` documentation explains its responsibility and entry points used outside that module, including interfaces for parents and siblings as well as any external interfaces.
`//! # Interface` lists the types and functions the root exposes or re-exports in normal builds, including types, visibility, function signatures, and definition links. Update it whenever an entry point changes. Link to definitions of supporting types used in signatures where necessary.
Do not exhaustively copy child module internals, fields, enum variants, or test-only items. Document public type methods, JSON representation and fallback contracts, and ownership, locking, or ordering requirements at their definitions. Comments describe current behavior rather than change history.
This manual list compensates for the pinned rustdoc's inability to show restricted re-exports as façade interfaces in this module arrangement.
Name links open the defining rustdoc page, whose **Source** link opens the implementation.
CI treats `rustdoc::broken_intra_doc_links` as an error to catch broken links, but does not check that signatures written in comments match the code.

## Privileged helper boundary

One Cargo package builds/installs `procinsh` and `procinsh-privileged-helper`;
`default-run = "procinsh"` preserves `cargo run`. Cargo provisions no capabilities.
The sibling helper is separately executed with Command, never a forked copy.

The main executable owns HTTP, routing, middleware, SSE, JSON formatting and
per-viewer system snapshot delta encoding. Its module closure excludes all
acquisition modules. Before starting any threads, it drops real/effective/saved
UID/GID, supplementary groups and all effective/permitted/inheritable/ambient
capabilities. Sudo launches use SUDO_UID/GID; direct root launches use 65534.
`no_new_privs` is set after helper exec and before Tokio/HTTP startup: setting it
before exec would suppress the helper's file capabilities. Both disable dumps.

### Ownership inventory

| Helper modules | Privileged operations and resources |
| --- | --- |
| `process/{identity,procfs,discovery,details,threads,maps}` | Protected procfs, PID/start-time checks, summaries/environment |
| `process/{fds,socket_metadata,socket_namespace,sockets}`, `netlink` | pidfds, pidfd_getfd duplicates, namespace handles, setns and complete diagnostic socket queries |
| `process/snapshot/{ptrace,memory,registers,capture}` | Ptrace guards, process_vm_readv, bounded register/stack/code capture |
| `process/snapshot/{perf,sample,unwind*,symbol}` | Perf FDs/mmaps, unwinding, identity-verified ELF descriptors and bounded in-memory DWARF |
| `process/monitoring` | Per-viewer samplers, histories, cancellation/cleanup |
| `system/{activity,ipc,sched,files,signals}` | BPF loading/attachment, links/maps and polling |
| `system/{snapshot,hardware,ipoib,resolver}` | Protected process/device acquisition and complete snapshots |

The helper intersects acquired capabilities with DAC_READ_SEARCH, SYS_PTRACE,
SYS_ADMIN, PERFMON and BPF, clears inheritable/ambient sets, and sets
`no_new_privs`. It never binds HTTP. Missing capabilities reduce coverage;
operation permissions still depend on kernel/namespace policy. ELF parsing
uses verified open descriptors and bounded sections, without loading external
DWO/DWP paths; unsupported files retain raw-frame fallbacks. Symbol resolution,
unwinding and disassembly finish in the helper before results cross IPC.

### Launch, protocol and lifetime

The default helper is next to `current_exe()`. Absolute `PROCINSH_HELPER` can
select a managed installation. PATH is never searched. Every path component
must be root-owned, without group/world write access or symlinks, and the helper
must be a regular file. An administrator able to replace protected files is
trusted. Command clears the environment and uses null stdin. Only stdio and a
private socketpair endpoint at FD 3 survive exec; no acquisition FDs exist in
main. The helper requires non-root IDs and validates SO_PEERCRED against its parent PID/real UID, sets
PDEATHSIG, checks its parent again and reports PID/package version; main matches
these against its Command child. There is no public IPC socket or SCM_RIGHTS.

Closed requests list processes, read specific process resources, observe
process/system activity, poll or cancel. None accepts paths, arbitrary addresses,
syscall numbers or caller-controlled limits. The helper checks PID/start time
independently before acquisition and after resource reads. Namespace readers
retain existing pinned-namespace validation. Only structured data/events cross
IPC. System delta selection remains in the application.

Four-byte frame lengths bound requests to 1 KiB and replies to 32 MiB, including
serialization allocation. Frame I/O and client exchanges have 30-second
deadlines. One dispatcher bounds in-flight work, a 64-entry client queue bounds
waiting requests, and monitors limit viewers to 32 per kind. Sessions expire
15 seconds after their last poll. Drop queues Close; expiry covers cancellation
during setup or a full queue. Procfs maps/status text is capped at 16 MiB and per-process inspection at
1024 threads. Oversized targets report an error. Existing capture/queue/history
limits and observation rates remain. Malformed/truncated frames and deadlines close IPC. Oversized replies
return bounded errors. A missing/crashed helper leaves UI/config available;
inspection returns 503. Restart main to establish a new helper connection.
HTTP input cannot select or launch replacement executables.

Normal shutdown closes IPC, cancels subscriptions, joins collectors and reaps
the helper; a 10-second parent guard bounds this. Parent death/disconnection
independently stops the helper. Process termination releases kernel tracing,
perf and BPF resources. Kernel calls/third-party parsers have no general hard
wall-clock bound; deadlines isolate HTTP and ptrace guards/process exit cleanup
remain essential.

### Installation, upgrades and containers

Never grant capabilities to main or user-writable Cargo output. Remove old
main grants with `sudo setcap -r /path/to/old/procinsh`. Build/install Cargo as
an ordinary user and put both executables in a protected directory. In a
checkout:

```sh
cargo build --release --locked
helper=$(sudo bash scripts/install_helper.sh target/release/procinsh-privileged-helper)
"$(dirname "$helper")/procinsh" --listen 127.0.0.1:9090
```

For Cargo installation pass `~/.cargo/bin/procinsh-privileged-helper` instead.
The installer hashes both binaries, stages root-owned copies, grants only the
helper capabilities and renames the directory into
`/usr/local/libexec/procinsh/<pair hash>/`. Both executables live together.
Existing pairs remain immutable. Switch a launcher to the new main after
installation succeeds; rollback selects the previous pair. Retain old pairs
until unused; administrator maintenance can remove them. Never overwrite an
active pair.

Review the script before running it with sudo. Automatic deployment uses an
administrator-owned copy at `/usr/local/sbin/procinsh-install-pair` and a reviewed
sudo policy. Never allow passwordless root execution of a user-writable script.
Preview scripts atomically replace launchers selecting protected pairs, retaining
the previous launcher for rollback. Their services run as the ordinary user.

`./scripts/dev_run.sh` installs a protected debug pair and runs its ordinary
main. It requires administrator provisioning rights. Preview with
`./users/akawashiro/scripts/preview_bind.sh run ./scripts/dev_run.sh PORT`.

Container images must include both binaries, root-owned and unwritable by the
runtime user. Grant required file capabilities only to helper, with matching
container bounding capabilities, BTF access and security policy. Container
`no-new-privileges` before exec prevents file-capability acquisition; main sets
its own restriction after helper startup. Tests retain default seccomp/AppArmor,
provision the pair inside Docker and directly execute the built main. Do not
weaken kernel security settings to enable inspection.

### Architectural guard and verification

`python3 scripts/check_privilege_boundary.py` walks the main/library module
closures against an explicit source allowlist. Helper-private modules cannot
enter main. Privileged wrappers, FFI and libc imports outside two credential/exec
modules are rejected. Those modules have explicit libc/syscall allowlists;
syscall aliases are rejected. Guard self-tests cover wrappers, FFI and aliases.
Self-tests also inject a helper-private module into main and require rejection.
Exception changes require reviewing these allowlists and documenting their role.
The guard is a review aid, not a proof for arbitrary macros, dependencies or
FFI aliases. The OS boundary is the capability-free HTTP process.

Docker ordinary runs all Rust tests, IPC framing/deadlines, PID reuse,
cancellation/disconnection, HTTP/SSE contracts, actual per-thread capabilities,
executable separation, descriptor ownership and parent/helper crashes. Kernel
and browser suites exercise live sensors and UI delivery. Run
`bash scripts/docker_test.sh ordinary`, `checks`, `kernel`, and `browser`.

Privilege separation does not authenticate HTTP: a compromised main can invoke
any allowed helper operation. The helper remains a sensitive privileged parser
and collector. Restrict network access accordingly.

## HTTP API

A process identity in JSON is `{ "pid": 123, "start_time_ticks": 456 }`. Addresses are hexadecimal strings to preserve precision in JavaScript.

APIs operating on a target require its identity on every request. No prior selection or SSE connection is necessary. Below, “identity query” means the required `pid` and `start_time_ticks` parameters.

| Method / path | Arguments | Return value |
|---|---|---|
| `GET /api/config` | None | Version, update interval `interval_ms`, history duration `history_seconds` |
| `GET /api/processes` | None | List of process identities, names, users, CPU/RSS, and other information |
| `GET /api/processes/observation` | Identity query | On-demand process observation; CPU usage and per-second rates are null |
| `GET /api/processes/threads` | Identity query | Array of on-demand thread observations; CPU usage is null |
| `GET /api/processes/maps` | Identity query | `process_id`, maps/smaps, rollup, capture time, capture error |
| `GET /api/processes/fds` | Identity query | FDs, candidate peers, shared owners, scan warnings |
| `GET /api/processes/environment` | Identity query | Environment variable names/values and retrieval information |
| `GET /api/processes/auxv` | Identity query | Auxiliary vector tags/values and resolved references |
| `GET /api/processes/events` | Identity query | SSE `observation`: target summary, latest observation, threads, history, maps, exit state |
| `GET /api/system/events` | None | SSE `snapshot`, `activity`, `gap`: structure, CPU/IPC/file I/O/signal activity, delivery gaps |

Missing or malformed identity queries return 400. PID must be a positive integer. Single GET requests, including observation/threads/maps, return 410 when they detect target exit or PID reuse. Other observation failures generally return 422; blocking task failures return 500.

SSE keeps a connection open with `Content-Type: text/event-stream`, using `event:` for the event name and `data:` for JSON. Immediately after connecting, `GET /api/processes/events` sends `observation` and `GET /api/system/events` sends `snapshot`. Keep-alive messages are not data updates.

### `GET /api/processes/events`

This endpoint sends SSE `observation` events. Supply an identity query such as `?pid=123&start_time_ticks=456`. The connection starts with an initial observation, then sends the entire target state below on each periodic observation (one second by default). Events include history and retained maps every time, rather than deltas. The entire event is never null.

Each connection collects independently and retains the latest 60 seconds of history starting at connection time. Multiple tabs for the same process do not share observations or history. Reconnecting starts a new history. Detail SSE allows at most 32 connections; excess connections return 429 and new connections during shutdown return 503.

| Field | Contents |
|---|---|
| `summary` | `identity` (PID/start time), name, executable, command line, real/effective users, state, CPU usage, RSS, thread count |
| `observation` | Latest `timestamp`, `process_id`, `cpu_percent`, `rss_bytes`, `vms_bytes`, cumulative minor/major faults and context switches, cumulative read/write bytes in `io`, per-second increments in `rates`, `threads`, CPU number, nice, priority; null if unavailable |
| `observation.threads` | TID, name, state, CPU number/usage, priority, nice, scheduler, affinity, cumulative context switches |
| `live_samples`, `sampling_error` | Latest registers, stack, instructions, sample age, loss counts per TID, and sampler errors |
| `history` | Array of `timestamp`, `cpu_percent`, `rss_bytes`, `vms_bytes` for the last 60 seconds |
| `maps` | Virtual memory region start/end, permissions, file offset, device/inode, path, RSS/PSS |
| `maps_captured_at`, `maps_error` | Map capture time and error; time is null before capture, error is null when there is no error |
| `rollup` | Aggregated RSS, PSS, private bytes; null if unavailable |
| `exited`, `error` | Target exit flag and observation error; error is null when absent |

Times are milliseconds since the Unix epoch, memory sizes are bytes, and 100% CPU means one core. In `rates`, faults and context switches are counts/second; reads and writes are bytes/second. Initial CPU usage without a prior observation, uncomputable rates, and unavailable optional values are null, distinct from zero. `summary` describes the process when the connection starts; continuously updated values are in `observation`.

Maps are captured about every five seconds, so their capture time differs from the latest observation time. On target exit, the server sends a final state with `exited: true` and ends the stream. Registers, call stacks, and disassembly arrive in `live_samples`. FD details, environment variables, and auxv use separate APIs.

### `GET /api/system/events`

This endpoint sends the SSE events below. `captured_at` is milliseconds since the Unix epoch; `window_ms` is the aggregation window in milliseconds.

The connection itself is a viewing session. At most 32 simultaneous connections are allowed; excess connections return 429 and new connections after application shutdown return 503.

| Event | Contents and timing |
|---|---|
| `snapshot` | Retained structure immediately after connection, structure updates separated by about a one-second wait, and resynchronization after delivery loss; apply as a full replacement or delta according to `kind` |
| `activity` | Activity aggregation about every second: `captured_at`, `window_ms`, `cpu`, `ipc`, `files`, `signals`, `status` |
| `gap` | `{dropped_frames: count}` when a subscriber falls behind, followed by the latest `snapshot`; lost activity is not resent |

A per-connection `SnapshotEncoder` converts `SystemSnapshot` from `system/snapshot.rs` to JSON. `ProcessSnapshot` represents a process; `FdEndpoint` a process FD endpoint; `FdRelation` a socket, pipe, or shared-ownership relationship; and `HardwareDevice`, `HardwarePath`, and `DeviceHandle` hardware information.

Top-level `snapshot` fields include `kind`, `sequence`, `captured_at`, `processes`, `warnings`, `inspected_processes`, `inspected_fds`, FD relations, and hardware data. `kind` is `full` or `delta`; `sequence` increases with each delivery within a connection. A `delta` includes the preceding `sequence` as `base_sequence`. Structure may be empty before the first collection.

The first update, resynchronization after a `gap`, and structure updates at least 60 seconds after the previous `full` send a `full` snapshot, replacing the entire structure including all maps and FD relations. Other updates use `delta`. Receivers check `base_sequence` against their retained structure's `sequence` before applying a delta. See SPACE's [mergeSnapshot](web/src/space/data.ts).

- `processes`: always includes every current process's `identity`, `parent_id`, name, real/effective users, `maps_epoch`, and `maps_error`. Remove identities absent from the list. `parent_id` is null if unknown; map retrieval failures include an error.
- Maps: `maps` replaces a process's maps; `maps_delta` applies `upsert` additions/updates and `remove` deletions keyed by start address. If both are omitted in a `delta`, retain maps and `maps_epoch` for the same process identity. New identities include `maps`.
- FD relations: `fd_relations` replaces all relations; `fd_relations_delta` applies `upsert` and `remove` keyed by connection ID. For both maps and FD relations, replacement is used when full JSON is smaller than a delta.
- Each FD relation: connection ID, endpoints `endpoint`/`peer`, label, socket information, `candidate`, `shared`. Endpoints contain process identity, FD, FD count, resource, kind, and access. `peer` is null without a local peer. `candidate` denotes a possible connection; `shared` denotes resource sharing, distinct from a uniquely identified communication peer.
- `socket`: protocol, state, local/remote addresses, network_peer, remote_hostname, remote_gid. Socket information and unavailable addresses/names may be null. `remote_gid` is the peer InfiniBand port's GID when the destination is an IPoIB neighbor in the collection thread's network namespace (same format as sysfs `gids/N`). The neighbor table is read with rtnetlink `RTM_GETNEIGH` only when an IPoIB interface and destination exist. IPoIB hardware addresses are 20 bytes; the final 16 bytes are the GID. `/proc/net/arp` is not used because it truncates the address. Expired neighbor entries and sockets in other network namespaces yield null.
- `warnings` and scan counts: warnings for unavailable data, truncated scans, and similar conditions, plus numbers of processes/FDs inspected.
- Hardware: `hardware_devices` and `hardware_paths` are small and sent in full each time. Handles use either full `device_handles` or `device_handles_delta` keyed by ID. Older servers omit these fields; receivers treat them as empty.
- Each `hardware_devices` entry: `address` (PCI address such as `0000:85:00.0`, or host bridge such as `pci0000:80`), `parent`, `role` (`host_bridge`, `root_port`, `switch_port`, `endpoint`), `kind` (`gpu`, `rdma`, `accelerator`, `nv_switch`; null for bridges), `label`, class/vendor_id/device_id as hexadecimal strings, driver, `numa_node`, `link` (`current`/`max` values of `{speed_gts, width}`), power_state/runtime_status, iommu_group, memory BAR `bars`, GPU `gpu` (vendor, model, uuid, NVIDIA device minor, DRM nodes), RDMA device `rdma`, and `netdevs`.
- Each `hardware_paths` entry: device pair `a`/`b`, `class` (`pix`, `pxb`, `phb`, `node`, `sys`, as in `nvidia-smi topo -m`), intermediate nodes `via`, and slowest negotiated link `bottleneck`.
- Each `device_handles` entry: ID, `process_id`, `device` (PCI address), `kind` (`nvidia`, `drm_render`, `drm_card`, `uverbs`, `umad`, `accel`), lowest FD number `fd`, and FD count `fd_count`.

Arrays in `activity` describe the current aggregation window, not the entire history.

| Field | Entry contents |
|---|---|
| `cpu` | `process_id`, `runtime_ns` (runtime in nanoseconds), `switches` (count), `running_threads` (count), `cpus` (running CPU numbers) |
| `ipc` | `process_id`, `resource`, `write`, `bytes`, `count`; true for send/write, false for receive/read; no payload |
| `files` | `process_id`, `file: {device: {major, minor}, inode, generation}`, `path`, `write`, `bytes`, `count`; inode is a decimal string, path is null if unavailable |
| `signals` | `timestamp_ns` (monotonic nanoseconds), `src_pid`, `dst_pid`, `signal` (number), `source_id`, `destination_id` (process identities including start times); observes `signal_generate`, not actual delivery or handler execution |
| `status` | `active`, CPU/IPC/files/signals sensor states, observation coverage descriptions, collection statistics such as `lost`, `files_lost`, `signals_lost`, `unresolved` |

Identifiers are structured. IPC activity and system snapshot FD `resource` values are `{kind: "pipe" | "socket", device: {major, minor}, inode: "…"}`; a memory map's `device` is `{major, minor}`. All inodes are decimal strings.

Socket protocol is `{kind: "tcp" | "udp", family: "ipv4" | "ipv6"}` or `{kind: "unix", socket_type: {kind, code?}}`; state is `{kind, code?}`. Unknown codes retain their numeric values. INET local/remote addresses are `{ip, port}`; UNIX paths use a separate `path`. FD access is `read`, `write`, `read_write`, or `unknown`; kind is `pipe`, `socket`, or `fifo`.

Thread scheduler is `{kind, code?}`; affinity is an array of inclusive `{start, end}` ranges, or null if unavailable.

Register mapping is `{pathname, readable, writable, executable, private}` or null; offset is a hexadecimal string or null; kind is the classification enum's snake_case name. Memory map permissions are booleans `readable`, `writable`, `executable`, and `private`, from which the UI constructs permission text.

Sensor status is `{state: "idle" | "observing"}` or `{state: "unavailable" | "error", message: "…"}`. Log levels are determined by the sensor state enum.

Activity arrays are empty when there is no matching activity or when a sensor is unavailable. An empty array alone does not establish that no activity occurred; also check sensor `state` values in `status` (observing, unavailable, error).

## Backend processing

### Common processing and state management

Axum extracts queries for each route and passes them to handlers. Responses passing Host, Origin, and Fetch Metadata validation receive no-store and security headers. Blocking process observation APIs run in `spawn_blocking`; mutexes protect state in `AppState`.

Targets use `{pid, start_time_ticks}`, not PID alone. Every request verifies the actual process identity and liveness. There is no server-wide selected target. Single-request APIs work without SSE and independently of requests from other processes or tabs.

### Configuration and process list

- `GET /api/config`: returns the package version, update interval specified at startup, and history retention in seconds.
- `GET /api/processes`: `ProcessScanner` scans `/proc` on each request and returns process identities, names, commands, users, memory sizes, and other information. CPU usage is calculated from the difference against the previous collection; it is null when no difference is available, including the first collection.

### On-demand observations, threads, and maps

These APIs validate the identity and read `/proc` on each request. They do not use state retained by SSE connections or stop the target with ptrace.

- `GET /api/processes/observation`: returns one observation of CPU, RSS/VMS, faults, I/O, context switches, threads, and other fields. CPU usage and `rates` are null because there is no previous observation for comparison.
- `GET /api/processes/threads`: returns the threads from a single observation. Per-thread CPU usage is null.
- `GET /api/processes/maps`: reads maps/smaps and rollup and returns them with capture time, capture errors, and process_id. Identity is checked before and after capture.

Use SSE for continuous CPU usage, per-second increments, and history. Ordinary `/proc` reads do not stop the target, and individual fields are not captured at precisely the same instant.

### FDs and peers

`GET /api/processes/fds` verifies identity and liveness, then collects pipe/FIFO/socket FDs, access direction, candidate peers, and owners sharing the same resource. UNIX domain socket peers are discovered with socket diagnostics. TCP/UDP candidates are found using information from the target process's network namespace. Shared owners and communication peers are distinct; no reads consume data. Scans are limited to three seconds, 100,000 FDs, and 8,192 matching FDs; results report truncation and similar conditions.

See [fds.rs](src/http_server/process/fds.rs) for FD collection, candidate matching, and searches for processes/FDs owning candidates. UNIX domain socket peer inode retrieval is in [socket diagnostics](src/http_server/process/sockets.rs).

Socket types are collected independently of peer identification and shown in SPACE connection details and process FD lists. Collection uses Netlink, RAW, and packet procfs tables as well as TCP, UDP, and UNIX tables. [SocketMetadataReader](src/http_server/process/socket_metadata.rs) temporarily duplicates socket FDs with `pidfd_open`/`pidfd_getfd` and reads `SO_DOMAIN`, `SO_TYPE`, `SO_PROTOCOL`, `SO_ACCEPTCONN`, and `getpeername`. Type, listening status, and peer presence are handled separately. Successful direct observations are shared within each collection by device/inode. Process start times and socket device/inode are validated; pidfds are retained per process during a scan. Duplicated FDs are closed immediately. Collection does not send/receive data, alter socket settings, or stop targets. Pidfd duplication requires ptrace permission; on insufficient permission or unsupported kernels, types obtained through procfs are retained.

The process FD list shows known types even for unconnected sockets, such as `TCP`, `NETLINK KOBJECT_UEVENT`, or `VSOCK STREAM`. API protocols other than TCP, UDP, and UNIX use `kind: "other"` and Linux `family`, `socket_type`, and `protocol` numbers, retaining unrecognized values. The Netlink procfs table alone does not identify socket type, so `socket_type` is null in that case. Unknown connection state is `state: null`; neither state nor peer PID is inferred from type. A socket with unknown type appears as `Socket · Type unavailable` and is included in scan warnings.

FDs at both ends of system snapshot relations and entries in the process FD API return `connection_status`. For sockets this is `listening` (nonzero `SO_ACCEPTCONN`), `unconnected` (not listening and `getpeername` returns `ENOTCONN`), `peer_present` (successful peer query), or `unknown` (unavailable/unsupported); non-sockets use null. The existing `state` independently retains TCP/UNIX state. When direct observation is unknown, only positive evidence from an explicit table LISTEN, peer inode, or meaningful TCP/UDP peer address supplements it. Zero peer addresses, CLOSE, or UNIX state names alone do not establish an unconnected socket. `peer_present` includes unnamed UNIX sockets and Netlink default destinations; it does not identify a peer process.

SPACE excludes relations when either socket FD is `listening` or `unconnected`, including shared-FD lines. IPC events for excluded sockets produce neither particles nor source-port glow. Sockets remain excluded during connection establishment while peer queries return `ENOTCONN`; communication through `sendto`/`sendmsg` on unconnected UDP is also absent from SPACE. `unknown` and missing fields from older servers remain visible. APIs and received raw snapshots retain all FDs; state changes update selection, particles, and retained Netlink displays.

Rust tests for pidfd type retrieval and separate network namespaces run with `bash scripts/docker_test.sh ordinary`. Missing observation permissions or kernel features cause failure.

UNIX diagnostics use a network namespace FD opened after checking the target PID and start time. For a different namespace, a short-lived dedicated thread calls `setns(CLONE_NEWNET)`, creates a diagnostic Netlink socket, returns its FD to the caller, and exits. Requests and responses use the socket belonging to that namespace; HTTP and collection threads keep their own namespaces. SPACE groups discovered namespaces by device/inode and diagnoses each once per scan. The total diagnostic budget across namespaces is 500 ms. The process FD list uses the shorter of 750 ms and its overall three-second deadline. If diagnostics fail, existing procfs information is retained and warnings include the target PID and failure reason.

Switching network namespaces requires `CAP_SYS_ADMIN`. [dev_run.sh](scripts/dev_run.sh) also grants this capability. Only the protected helper receives capabilities; see the [deployment guide](#privileged-helper-boundary). The application does not modify sudoers or kernel settings.

### Hardware

The system snapshot uses [system/hardware](src/http_server/system/hardware.rs) to read GPUs, RDMA NICs, accelerators, NVSwitches, and their upstream PCIe bridges up to host bridges from sysfs. This requires no privileges.

- Discovery: upstream chains come from each device's real path under `/sys/bus/pci/devices`, such as `/sys/devices/pci0000:80/0000:80:03.4/0000:85:00.0`. Devices with `infiniband/` or class 0x0c06 are RDMA NICs; classes 0x0300/0x0302 are GPUs (excluding ASPEED/Matrox BMCs); class 0x12 denotes accelerators; NVIDIA class 0x0680 denotes NVSwitch. Only their upstream bridges are included.
- Attributes: reads class, vendor, device, driver, numa_node, `current/max_link_speed`, `current/max_link_width`, power_state, `power/runtime_status`, iommu_group, and BARs from `resource`. It does not open `config` or BAR resize files, which could trigger runtime resume. Link speeds are not read for runtime-suspended devices. NVIDIA GPU model, UUID, and device minor come from `/proc/driver/nvidia/gpus/<address>/information`; vendor libraries such as NVML are not used.
- Paths: device pairs are classified from common upstream nodes as in `nvidia-smi topo -m`. Different host bridges yield `node` on the same NUMA node or `sys` otherwise (unknown NUMA counts as the same node when only one NUMA node is online). A shared host bridge ancestor yields `phb`; traversing at most one PCIe switch (upstream port and its downstream ports) yields `pix`, otherwise `pxb`. The negotiated link with the lowest bandwidth after encoding across all hops is the `bottleneck`.
- Processes: FD scans record character device rdev and derive PCI address/type (`drm/renderD*`, `drm/card*`, `infiniband_verbs`, `infiniband_mad`, `accel`) from `/sys/dev/char/<major>:<minor>` symlink targets. `/dev/nvidiaN` has no sysfs node, so GPUs are matched using the `nvidia` major number in `/proc/devices` and device minor. Control devices such as `nvidiactl` are excluded. FDs with the same process, device, and type are grouped.
- Traffic: GPU/NIC DMA is not observed, so PCIe links have no particles.

### Environment variables and auxiliary vectors

Both are retrieved on demand after identity/liveness checks, rather than recollected as part of periodic observations.

These APIs read files exposed by procfs. Only auxv string references additionally read target process memory with `process_vm_readv`.

- `GET /api/processes/environment`: reads up to 1 MiB from procfs [`/proc/<pid>/environ`](https://man7.org/linux/man-pages/man5/proc_pid_environ.5.html), splitting NUL-delimited entries at the first `=` into names and values. Duplicate names, empty values, and `=` within values are preserved. This usually reflects the environment region at exec time, not every subsequent change. See [file reading](src/http_server/process/details.rs) and [environment parsing](src/http_server/process/details.rs).
- `GET /api/processes/auxv`: determines 32/64-bit mode from the ELF header of `/proc/<pid>/exe` and reads up to 64 KiB from procfs [`/proc/<pid>/auxv`](https://man7.org/linux/man-pages/man5/proc_pid_auxv.5.html). It parses tag/value pairs, handling known and unknown tags. String references for `AT_EXECFN`, `AT_PLATFORM`, and `AT_BASE_PLATFORM` are resolved with `process_vm_readv`, up to 4,096 bytes each. Numeric values remain when references cannot be read. Big-endian ELF is unsupported. See [ELF header and auxv reading](src/http_server/process/details.rs), [string reference resolution](src/http_server/process/details.rs), and [memory reading](src/http_server/process/memory.rs).

### Initial/periodic snapshots and live sampling without stopping the target

Before delivering the initial watch value, each SSE collector enumerates threads, opens ordinary perf events, then independently stops each TID with `PTRACE_SEIZE` → `PTRACE_INTERRUPT` → `waitpid(__WALL)`. It captures registers with `PTRACE_GETREGSET(NT_PRSTATUS)` and up to 16 KiB of stack from RSP (bounded by the containing mapping) with `process_vm_readv`, then detaches on both success and failure. Initial/10-second ptrace captures and continuous perf share `RawSample`, unwinding, symbol resolution, and register classification. Ptrace CPU is null. In addition to the initial capture, ptrace recaptures all TIDs 10 seconds after capture completion, including TIDs receiving perf updates. Delays do not trigger catch-up captures; failures also wait 10 seconds before retrying. Unavailable TIDs retain their previous samples while perf monitoring continues. Periodic results are delivered on the regular observation cycle.

Each SSE collector opens a software CPU-clock event per TID with `perf_event_open(pid=tid, cpu=-1)`, following CPU migration. The initial interval is 10 ms of running user CPU time (about 100 Hz), with a 16,384-byte user stack dump. Transitions to waiting use an additional `PERF_COUNT_SW_CONTEXT_SWITCHES` event (period 1, stack dump 16,384 bytes). With `context_switch` and `sample_id_all` enabled, the following SWITCH_OUT record in the same ring is checked for TID, CPU, time, and PREEMPT flag to distinguish voluntary/preempted switches. Associations are discarded without a matching sample or on loss, throttle, or switch-in. The latest samples from both events are compared by monotonic time; only newer samples are accepted. TID/TIME/CPU/REGS_USER/STACK_USER are read from the ring buffer, retaining only the latest values. Rings are drained and thread additions/exits checked about every 50 ms. TID and start time detect reuse, and process identity is validated before and after capture.

The 18 registers and stack come from the same perf sample or ptrace stop. Framehop unwinds up to 256 frames using only sampled RSP/RBP/RIP and captured bytes. Each sampler retains module and rule caches, rebuilding them when executable mappings change. ELF retrieval uses existing device/inode validation and caching, correcting load bias for PIE, non-PIE, and shared libraries. It reads `.eh_frame`/`.eh_frame_hdr` (a CFI index if the header is missing) and `.debug_frame`; when metadata is unavailable, framehop falls back to frame pointers. ELF files with `.debug_frame` prefer it to avoid application CFI being hidden by CRT-only `.eh_frame`. Existing symbol, source, and inline-frame resolution is reused.

`unwind_stop` reports normal completion, the 256-frame limit, insufficient stack snapshot coverage, out-of-range addresses, failed module retrieval, or missing module metadata. The last two note fallback use. Even with a section present, missing/unsupported CFI for a particular PC can trigger framehop's internal fallback, which cannot be distinguished in that detail. Unwinding does not read additional live stack data or stop the target with ptrace. Failed modules are not fetched again until executable mappings change.

SSE `live_samples` includes TID, `sampled_at` (Unix ms), `sample_age_ms` (monotonic clock), CPU, `lost_samples`, registers, call_stack, disassembly, unwind_stop, and error. Before capture, time is null and the UI shows Waiting for sample; captured values show age in the Threads list. Sleeping/blocked threads attempt updates through ptrace every 10 seconds, retaining the last values otherwise. These are saved user-space states, not the current values of sleeping threads. Threads sleeping before observation can be captured in the initial delivery if ptrace is allowed. An initial failure waits for the next execution/switch-out or a ptrace retry after 10 seconds. The Registers panel shows only the register table. Capture source and voluntary/preempted distinctions remain internal and are not exposed through the GUI or HTTP API. Simultaneous state across all threads is not guaranteed. Insufficient perf permissions appear as thread errors in Threads. Collector errors are retained in API `sampling_error` while ordinary observations continue.

The Disassembly panel shows instructions and retrieval errors. Disassembly is best effort, reading after sampling from the sampled RIP using `process_vm_readv`. Iced-x86 decodes up to 256 bytes and 32 instructions. JIT/self-modifying code bytes may not match the sampled RIP's state. 32-bit compatibility mode is unsupported.

PERF_RECORD_LOST, ring overruns, and invalid records count as loss. RAII releases FDs/mmap mappings on thread exit, target exit, SSE disconnection, and server shutdown. CAP_SYS_PTRACE and process_vm_readv are used for ptrace snapshots, auxv, and live disassembly.

### Process detail monitoring over SSE

`GET /api/processes/events` reserves a connection slot, validates identity, and captures the initial observation and maps. It creates a dedicated OS thread and watch channel for the connection, observes at the configured interval, and updates the latest 60 seconds of history. Maps/smaps refresh about every five seconds.

The response stream owns an RAII guard that releases the connection slot and sets a stop flag even if dropped unread. In-progress collection stops after completing. Application shutdown also stops collection and joins collector threads. Target exit sends a final state and ends collection and streaming.

Each connection has its own watch channel. Slow subscribers receive the latest value without accumulating old states. Keep-alive is every 10 seconds. Network failure detection may be delayed, allowing collection to continue until disconnection is detected.

### System-wide state/structure and viewing sessions

- `GET /api/system/events`: checks the connection limit while incrementing the count and starts the collection worker on the first connection. The response stream owns an RAII guard that decrements the count on completion/drop, including unread responses.
When the last connection closes, collection pauses and sensors are released when the worker next checks state. The worker thread remains until application shutdown and resumes collection on reconnection. Network failure can delay server-side disconnect detection; there is no upper bound on the time until collection stops.

After completing structure collection and delivery, the worker waits about one second before collecting again. The actual interval includes processing and waiting. Global FD scans are limited to 100,000 FDs and four seconds, mappings to 4,096 per PID, and the connection graph to about 20,000 connections. Process parent/child relations, maps, pipes/sockets, and network peers are discovered without stopping targets; unavailable data, truncation, and loss are represented as state. Network peer name resolution is cached.

### System-wide activity collection and SSE delivery

`GET /api/system/events` registers a viewer and subscribes to a broadcast channel. It first returns retained structure as `snapshot`, then streams structure and activity. Sensor states and collection statistics are in `activity.status`. Disconnection or delivery completion unregisters the viewer; application shutdown ends the stream.

When a subscriber falls behind, it receives `gap` and the latest `full` snapshot; lost activity is not replayed. Keep-alive is every 10 seconds. Activity is aggregated/delivered about every second, with the actual window in `window_ms`.

All sensors use CO-RE eBPF. CPU scheduling, IPC, and file I/O use independent eBPF programs with independent loading, state, and release. SPACE uses delivered `CpuActivity` for CPU glow. Scheduler events aggregate in kernel maps, read by userspace during activity aggregation.

| Sensor | Backend observations and limits | eBPF source |
|---|---|---|
| CPU | Aggregates runtime and running CPUs with `sched_switch` | [sched.bpf.c](src/http_server/system/sched.bpf.c) |
| IPC | Observes pipe read/write and socket send/receive results; no payload reads or MSG_PEEK counts; excludes splice/sendfile, some io_uring, and workers without known attribution | [ipc.bpf.c](src/http_server/system/ipc.bpf.c) |
| File I/O | Observes successful byte/count results of VFS read/write and vectored I/O, including page-cache access; excludes mmap, io_uring, splice/sendfile, and physical disk transfer volume | [files.bpf.c](src/http_server/system/files.bpf.c) |

File paths are captured at operation time; unavailable paths use identifiers such as device/inode. Unavailable BPF hooks produce per-sensor reasons in the status API and logs while collection of available information continues. Required kernel features and permissions vary by sensor.

## Frontend processing

The list, process detail, and SPACE pages have separate HTML and TypeScript entry points, with ordinary links between them. Each page manages its own state and connections. TypeScript loads as ES modules; every page's title is `procinsh`.

### Page entry points and file responsibilities

| Location/file | Responsibility |
|---|---|
| [list/index.html](web/list/index.html), [list/style.css](web/src/list/style.css), [list/app.ts](web/src/list/app.ts) | `/list` HTML/CSS and startup; `app.ts` connects data, search, and rendering; Vite bundles `list/app.ts` |
| [list/data.ts](web/src/list/data.ts) | Fetching list/update interval, observation state, periodic refresh, cancellation on page departure, stale-response rejection |
| [list/search.ts](web/src/list/search.ts) | PID/name/command-line search, CPU/RSS/PID sorting, interaction events |
| [list/renderer.ts](web/src/list/renderer.ts) | DOM updates for the list table, identity-bearing detail links, counts, retrieval errors |
| [process/index.html](web/process/index.html), [process/style.css](web/src/process/style.css), [process/app.ts](web/src/process/app.ts) | `/process/{pid}` HTML/CSS and startup; `app.ts` connects networking, rendering, and interactions; Vite bundles `process/app.ts` |
| [process/data.ts](web/src/process/data.ts) | URL PID/start-time validation, identity resolution, detail SSE, observation/sample/additional GET state, periodic fetching for open panels, stale-notification/response rejection |
| [process/renderer.ts](web/src/process/renderer.ts) | DOM updates for process summary, metrics, exit/error state, memory maps |
| [process/selection.ts](web/src/process/selection.ts) | Retaining/switching thread selection, thread table and selected-thread attributes |
| [process/samples.ts](web/src/process/samples.ts) | DOM updates for selected-thread registers, Call Stack, disassembly |
| [process/history.ts](web/src/process/history.ts) | CPU/RSS graphs, axes, and legends drawn from history on Canvas 2D |
| [process/details.ts](web/src/process/details.ts) | Environment, Auxiliary Vector, Pipe / Socket DOM updates and panel toggle events |
| [process/search.ts](web/src/process/search.ts) | Searching retained environment variables/FDs and input events |
| [space/index.html](web/space/index.html), [space/style.css](web/src/space/style.css), [space/app.ts](web/src/space/app.ts) | `/` and `/space` HTML/CSS and startup; `app.ts` connects data, scene, and interaction components; Vite bundles `space/app.ts` |
| [space/types.ts](web/src/space/types.ts), [space/contracts.ts](web/src/space/contracts.ts) | Shared layout/activity/selection/rendering data types and component input/callback contracts |
| [space/model.ts](web/src/space/model.ts) | Pure functions for process/resource identities, labels, UID colors |
| [space/data.ts](web/src/space/data.ts) | SSE, observation/activity state, structure-event merging, retained layout state; independent of Three.js/DOM |
| [space/process-layout.ts](web/src/space/process-layout.ts) | Pure functions for full parent/child tree layout and added-process placement preserving existing coordinates |
| [space/scene.ts](web/src/space/scene.ts) | Building/updating Three.js objects from layout data and releasing unused Geometry/Material resources |
| [space/renderer.ts](web/src/space/renderer.ts) | Render loop, activity animations, Canvas 2D text labels, FPS/render-resolution adjustment |
| [space/search.ts](web/src/space/search.ts) | PID/name filtering and search events |
| [space/selection.ts](web/src/space/selection.ts) | Selection/hover state, click/drag detection, scene hit testing |
| [space/camera.ts](web/src/space/camera.ts) | OrbitControls, camera bounds/projection, fit-all, focusing selection |
| [space/details.ts](web/src/space/details.ts) | DOM updates for selected process/connection/file details and process links |
| [shared/api.ts](web/src/shared/api.ts) | JSON fetching and API error handling shared by list/details |
| [shared/display.ts](web/src/shared/display.ts) | Formatting bytes, percentages, per-second rates, permissions, schedulers, sockets |
| [shared/dom.ts](web/src/shared/dom.ts), [shared/style.css](web/src/shared/style.css) | List/detail element creation and shared header/form/table styles |
| [shared/navigation.ts](web/src/shared/navigation.ts) | Process identity matching, API queries, detail links preserving start time |
| [shared/api-types.ts](web/src/shared/api-types.ts) | TypeScript types matching Rust JSON/SSE responses; no runtime input validation |
| [list/dom-types.ts](web/src/list/dom-types.ts), [process/dom-types.ts](web/src/process/dom-types.ts), [space/dom-types.ts](web/src/space/dom-types.ts) | HTML element ID/type mappings: `ListElements`, `ProcessElements`, `SpaceElements`; shared typed element lookup through `listElement`, `processElement`, `spaceElement` |
| [web.rs](src/http_server/web.rs) | Embedding HTML/CSS, generated JavaScript, vendor files in the Rust binary and serving them at their URLs |

TypeScript build settings are in [tsconfig.json](web/tsconfig.json), commands in [package.json](web/package.json). Generated HTML goes to `web/dist/{list,process,space}/index.html`, JS/CSS to `web/dist/assets/`. Browser routes `/`, `/list`, `/process/{pid}`, and `/space` serve the corresponding generated HTML; HTML references assets under `/assets/`. To view changes, build the web assets and Rust binary as described in [Build and runtime environment](#build-and-runtime-environment).

### Process list `/list`

[list/app.ts](web/src/list/app.ts) connects `ListDataStore`, search, and table rendering. See [`ListDataStore.start`](web/src/list/data.ts#L49)/[`ListDataStore.refresh`](web/src/list/data.ts#L23) for networking/state, [`matchingProcesses`](web/src/list/search.ts#L3) for search/sorting, and [`renderProcesses`](web/src/list/renderer.ts#L8) for DOM updates. Creating the data component does not start networking; data updates and searches calculate visible rows and update the table.

The list page contains only list DOM and does not open detail SSE.

| API used | Timing and purpose |
|---|---|
| `GET /api/config` | Fetch update interval on initialization or restoration from the page cache |
| `GET /api/processes` | Fetch list for initial display and periodic refresh |

The refresh interval is the greater of the configured interval and 1,000 ms. Refreshes are skipped while a list request is in progress. PID/name/command-line search and descending CPU/RSS or ascending PID sorting run in the browser against fetched data.

Process names link to `/process/{pid}?start_time_ticks={start_time}` using [`processUrl`](web/src/shared/navigation.ts#L4). Normal navigation opens details while preserving the observed `{pid, start_time_ticks}`. The header's Go to space view opens `/space`.

`pagehide` stops periodic refresh and cancels an in-flight list request. Restoring from the page cache refetches configuration/list and resumes refresh.

### Process details `/process/{pid}`

[process/app.ts](web/src/process/app.ts) connects data, rendering, and selection; handles page startup/shutdown; and preserves the identity-bearing URL. [`ProcessDataStore.connect`](web/src/process/data.ts#L158) establishes SSE; [`ProcessDataStore.acceptTarget`](web/src/process/data.ts#L200) → [`renderTarget`](web/src/process/renderer.ts#L22) applies received state. See [`ProcessDataStore.loadDetails`](web/src/process/data.ts#L120) for additional panels, [`drawHistory`](web/src/process/history.ts#L10) for history graphs, and [`renderLiveSample`](web/src/process/samples.ts#L43) for selected-thread samples.

`ProcessDataStore` is DOM-independent and retains observations by checking PID, start time, connection generation, and additional GET generation. Search filters fetched values; `ThreadSelection` retains the selected thread. State notifications update individual rendering components; history graphs receive history data and a Canvas.

| API used | Timing and purpose |
|---|---|
| `GET /api/processes` | At initialization, match PID and URL start time; resolve the PID's start time if absent from the URL |
| `GET /api/processes/events` | Connect with an identity query and update summary, observation, history, threads, maps, exit state |
| `GET /api/processes/environment`, `GET /api/processes/auxv`, `GET /api/processes/fds` | When opening each panel and every five seconds while open (skip while fetching) |

The detail page contains only detail DOM and does not periodically refresh the process list. At startup it fetches the list once, matches PID and URL `start_time_ticks`, and connects SSE. If start time is absent, it resolves the current identity and retains start time in the URL. Invalid start time, missing target, or identity mismatch produces a detail-page error with a list link. It never connects to a mismatched process. Additional GETs use the same identity query. Regular updates use SSE `observation`, not single observation/threads/maps GETs.

Received summaries/observations update metrics and the thread list; history draws CPU/RSS graphs on Canvas. Map display updates when capture time changes or capture errors occur. The thread list shows sample age, Waiting for sample before capture, and retrieval errors.

The latest sample for each thread is retained; the selected thread's registers, Call Stack, and disassembly are shown. Call Stack spans all detail-grid columns and shows frame addresses, symbols, source locations, inline frames, unwind stop reasons, and errors. Addresses in maps/registers/stacks/disassembly/auxv and strings such as environment variables are displayed with `textContent`.

Environment, Auxiliary Vector, and Pipe / Socket use a single GET whenever opened, then refresh only open panels every five seconds. A refresh is skipped while a request of the same kind is pending. Environment and Pipe / Socket searches use fetched data. Failed refreshes retain previous results when available and show an error.

The list link, header application name, and FD peer links use normal navigation. FD links also preserve observed start time. Each detail page has independent details, panel-open state, and searches. Connection generation/identity checks ignore stale SSE notifications. Additional GETs check request generation/identity and response identity to discard stale responses. Process exit and `pagehide` stop panel refresh and invalidate pending responses.

Connection loss displays an error; EventSource reconnects automatically with the same identity. A new observation clears the error, and history restarts for the new connection. It does not switch to another process with the same PID. Receiving `exited: true` closes the connection and retains the final state/exit indication. `pagehide` closes the connection; `pageshow` after page-cache restoration reconnects non-exited targets with the same identity.

#### List and process detail verification entry points

| Coverage | Test source | Command |
|---|---|---|
| List search/sorting/refresh cancellation; detail identity validation/SSE/additional GETs/selection | [page-data.test.ts](web/tests/page-data.test.ts), in-source tests in [list/search.ts](web/src/list/search.ts) and [process/data.ts](web/src/process/data.ts) | `bash scripts/docker_test.sh web` |
| CPU/RSS axes/scales, missing samples, rendering scale | [history.ts in-source tests](web/src/process/history.ts) | `bash scripts/docker_test.sh web` |
| List-to-detail navigation, extra panels, samples, mobile, page cache/exit | [browser.ts](web/tests/browser/browser.ts) | `bash scripts/docker_test.sh browser` after [browser test preparation](#browser-tests) |

### SPACE `/` and `/space`

SPACE shows system-wide processes, virtual address spaces, parent/child relations, IPC/network connections, and file I/O in 3D. [space/app.ts](web/src/space/app.ts) creates/connects components and handles page startup/shutdown. Data flows through `data.ts` → `scene.ts` → `renderer.ts`; search changes visibility, selection changes highlighting/details, and the camera changes the viewpoint.

`SpaceDataStore` in `data.ts` owns SSE, observation state, activity aggregates, and layout, providing numeric `{x, y, z}` coordinates. At the rendering boundary, `scene.ts` converts them to Three.js Vector3 and rendering objects. Creating data components does not start networking; layout/state updates can be tested with Node.js alone.

`treeLayout` and `stableLayout` in [process-layout.ts](web/src/space/process-layout.ts) calculate process positions. Input is `ProcessLayoutNode[]` (process/parent identities); output is `Map<string, TreePosition>` keyed by identity (`x`, `y`, normalized `depth`, `parent`). `treeLayout` handles initial layout and Rearrange; `stableLayout` preserves existing coordinates and places new processes during regular updates. Spacing is configurable through `xGap` (default `4.8`) and `yGap` (default `6.5`). Test layout without starting a server or rendering with `bash scripts/docker_test.sh web src/space/process-layout.ts`.

Components use shared contracts in [contracts.ts](web/src/space/contracts.ts) and [types.ts](web/src/space/types.ts), without importing each other's concrete types. `app.ts` passes structure, layout, and visible targets to the scene and explicitly provides selection to `updateSelection`. Structure/file updates proceed through selection retention, scene update, selection highlighting, particle retention, camera bounds adjustment, and detail display in that order.

Hit testing receives current Three.js objects and display data for each event and reports selection/hover to `app.ts` through callbacks. Rendering receives scene, camera, rendering data, and selection; before-render notifications let `app.ts` update OrbitControls and remove expired files about every second. The camera derives bounds and fog density from coordinates; `app.ts` passes fog density to the scene. The detail panel receives selection/display data and reports connection-selection actions through callbacks.

#### Receiving data and updating structure

On initial display or redisplay, `app.ts` calls [`SpaceDataStore.start`](web/src/space/data.ts), connecting EventSource to `GET /api/system/events`. The initial structure also arrives through SSE; the page is registered as a backend viewer only while connected. See [api-types.ts `SystemSnapshotUpdate`](web/src/shared/api-types.ts#L319)/[`SpaceActivity`](web/src/shared/api-types.ts#L352) for event types and the [HTTP API description](#get-apisystemevents) for contents.

| Event | Applied changes | Main implementation |
|---|---|---|
| `snapshot` | `full` replaces structure; `delta` merges into retained structure after checking `base_sequence` against retained `sequence`; applies maps/FD-relation deltas, preserving omitted maps for the same process identity | [`mergeSnapshot`](web/src/space/data.ts) → [`SpaceDataStore.replaceSnapshot`](web/src/space/data.ts) → [`buildScene`](web/src/space/scene.ts#L99) |
| `activity` | Updates CPU glow, IPC/network particles, recent file I/O | [`SpaceDataStore.ingestActivity`](web/src/space/data.ts) → [`activity`](web/src/space/renderer.ts#L292) |
| `gap` | Clears rendered particles and applies the following `full` snapshot | [`SpaceDataStore.connect` event handlers](web/src/space/data.ts) |

#### Layout of processes, peers, and files

[`SpaceDataStore.replaceSnapshot`](web/src/space/data.ts) builds layout state from received structure; [`buildScene`](web/src/space/scene.ts#L99) builds process boxes, memory regions, parent/child lines, and connection lines. Structure updates add/remove objects while preserving existing process/peer positions.

| Target | Layout and display | Main implementation |
|---|---|---|
| Processes and parent/child relations | Initial layout/Rearrange groups leaf children in one wrapping region. This region sits in a horizontal row alongside regions for sibling branches with descendants. Regions containing processes with more distinct socket peers sit nearer the center; within leaf regions, processes with more peers get central positions first. Peer counts use verified local connections for current identities, deduplicate multiple FDs to the same peer, and exclude shared FDs, candidates, external peers, and self-connections. Without processes having multiple peers, the leaf region occupies the first leaf's position in PID/start-time order. Branch regions do not stack vertically; separate root trees can wrap as whole units. Wrapped rows shift right by one process width each. Leaf wrapping can place nodes at different vertical positions despite equal depth. Regular updates retain positions and put new processes in free space near their parents, without enforcing horizontal branch separation | [`treeLayout`, `stableLayout`](web/src/space/process-layout.ts) |
| Virtual address spaces | Stack memory regions in address order, compress gaps, normalize height per process; equal heights across processes do not imply equal addresses | [`layoutMaps`](web/src/space/data.ts), [`regionColor`](web/src/space/scene.ts#L81) |
| User identity | Top/bottom box frames use real UID colors; vertical frames use effective UID colors | [`userColor`, `processColors`](web/src/space/model.ts) |
| IPC/network connections | Solid FD-relation lines: green tones for confirmed inter-process connections, pale ochre for candidates, pale blue-gray for shared FDs. Network peers are grouped by process/protocol/peer IP/port and placed above processes. IPoIB peer labels include “IB”; details show peer InfiniBand GID | [`buildScene`](web/src/space/scene.ts#L99), [`networkGroups`](web/src/space/data.ts), [`networkLayout`](web/src/space/data.ts) |
| Recently accessed files | File markers and connection lines below processes, updated in a group separate from structure | [`fileLayout`](web/src/space/data.ts), [`refreshFileScene`](web/src/space/scene.ts#L400) |
| Hardware | PCIe tree to the left (−x) of the lowest-PID process (usually systemd). Devices form a y-axis column closest to that process; root ports, switches, and host bridges extend left, with parents centered on children. Overlap with process boxes shifts the whole tree left. Hardware labels are placed before regular labels to keep them visible. GPUs are amber boxes, RDMA NICs purple boxes, bridges small platforms. PCIe links to bridges are floor strips, with width by lane count and color by generation; links below both endpoints' limits are orange. A line always connects each process's front face to each device it opens (one per process/device pair; amber for GPUs, purple for RDMA NICs; search-hidden processes excluded). Selecting that line selects the device. Selecting a device/process highlights related lines. Selecting a device highlights paths to other devices by class color and shows class/bottleneck. Details include ID, driver, NUMA, IOMMU group, BAR, GPU/RDMA information, downstream devices, paths, processes | [`hardwareLayout`](web/src/space/hardware.ts), [`buildHardware`](web/src/space/scene.ts) |

#### Activity display and render loop

[`SpaceDataStore.ingestActivity`](web/src/space/data.ts) updates activity data and rendering paths; [`animate`](web/src/space/renderer.ts#L338) updates particles and CPU glow on each `requestAnimationFrame`. The render loop stops while the tab is hidden.

| Display | Behavior | Main implementation |
|---|---|---|
| CPU glow | Strengthens while running and decays in about 500 ms after activity stops | [`cpuGlowLevel`](web/src/space/renderer.ts#L67) |
| IPC/network particles | Generated according to read/write direction and operation count; if no unique peer is identified, only the source port glows. `prefers-reduced-motion` reduces particle lifetime/count | [`edgeDirection`](web/src/space/data.ts), [`ipcParticlePlan`](web/src/space/renderer.ts#L48), [`SpaceDataStore.ingestActivity`](web/src/space/data.ts) |
| Netlink communication | Visible Netlink sockets except USERSOCK retain an orange hexagonal plate and connection below the process while present in snapshots, regardless of traffic. Duplicate FDs group by process/socket resource; counts are zero before communication. Only send/receive events emit particles: sends toward the plate, receives toward the process. No time/count expiry; removed when the socket disappears, its process exits, SSE resets, or state becomes listening/unconnected. Plates conceptually represent kernel-side interfaces; actual peer Netlink addresses are not observed | [NetlinkSocketStore](web/src/space/netlink.ts), [refreshFileScene](web/src/space/scene.ts) |
| File I/O | Read/write particles and retained-file byte/operation counts; retain for 30 seconds after last access, at most 32 per process and 512 total; remove exited processes' files | [`RecentFiles`](web/src/space/data.ts), [`fileDetails`](web/src/space/details.ts#L72) |
| Process/peer labels | Project 3D positions to screen coordinates and draw on a Canvas 2D separate from WebGL. Overlapping unselected labels retain the nearer one. Labels visible in the previous frame take priority, avoiding per-frame switching between similar depths. Visibility fades over about 150 ms, or switches immediately with `prefers-reduced-motion` | [`drawLabels`](web/src/space/renderer.ts#L284), [`placeLabels`](web/src/space/renderer.ts#L107) |

[`AdaptiveRenderScale`](web/src/space/renderer.ts#L12) adjusts rendering resolution using FPS measurements about every second. Three consecutive readings below 24 FPS lower pixel ratio by 10%; five consecutive readings at or above 45 FPS recover toward the original resolution. The upper bound is the initial device pixel ratio (capped at 1.5); the lower bound is 0.5, or the initial value if smaller. The header shows FPS and resolution percentage.

Signals use `raw_tp/signal_generate`, normalizing source/destination to thread group leaders and delivering only events whose start times match current structure. The kernel ring buffer is 256 KiB; delivery windows retain up to 1,024 events, counting overflow in `signals_lost`. Disable signal collection with `PROCINSH_DISABLE_SIGNALS=1 ./scripts/dev_run.sh ...`. Other observations and SPACE continue when signals are disabled, permissions are insufficient, or hooks are unavailable.

SPACE retains up to 256 signals with both endpoints visible. Purple lights and faint trails travel to destinations over about 1,700 ms; arrival glows at the destination base for 400 ms. At most 12 signal-name labels appear simultaneously. This animates observation events, not measured delivery latency. `prefers-reduced-motion` omits trails and reduces travel to 300 ms. Search, process disappearance, and SSE resets discard invalid displays.

#### Search, selection, and camera controls

Interactions run in the browser. Selection does not call detail APIs; retained structure/activity updates the right-hand details panel. Open process details uses [`processUrl`](web/src/shared/navigation.ts#L4) to preserve observed start time and navigate to `/process/{pid}?start_time_ticks={start_time}`.

| Interaction | Behavior | Main implementation |
|---|---|---|
| PID/process-name search | Filters rendered targets; Enter selects the first match and focuses the camera | [`visibleIds`](web/src/space/search.ts#L4), [search events](web/src/space/search.ts#L19) |
| Click/hover | Selects/describes processes, connection lines, network peers, Netlink plates, files; double-clicking a process focuses the camera | [`hit`, `edgeHit`](web/src/space/selection.ts#L75), [pointer events](web/src/space/selection.ts#L60), [`details`](web/src/space/details.ts#L138) |
| Drag/right-drag/scroll | Rotate/pan/zoom camera | [OrbitControls configuration](web/src/space/camera.ts#L6) |
| Fit all | Clears search/selection and fits the whole scene | [`fit`](web/src/space/camera.ts#L57), [button events](web/src/space/app.ts#L214) |
| Rearrange | Clears particles, rearranges processes/network peers/Netlink plates/files, and fits the scene | [`SpaceDataStore.replaceSnapshot`](web/src/space/data.ts), [button events](web/src/space/app.ts#L208) |

#### Connection and page lifecycle

See [`SpaceDataStore.start`](web/src/space/data.ts) for connection startup, [`SpaceDataStore.stop`](web/src/space/data.ts) for shutdown, and [`visibilitychange`/`pagehide`/`pageshow` handlers](web/src/space/app.ts#L234) for starting/stopping connections and rendering based on page visibility.

- Hiding the tab or `pagehide` stops SSE/reconnect timers and clears particles, CPU glow, Netlink plates, and files. Redisplay or page-cache restoration reconnects.
- Connection errors close the current EventSource, display an error, and create a new connection after three seconds only while visible. Successful connection clears the error; events from old connections are ignored.
- Structure parsing/merging failures close the connection and immediately reconnect while visible to obtain initial structure.
- Visibility changes, page departure, and page-cache restoration reset FPS measurements and consecutive-resolution-adjustment counters.

#### SPACE verification entry points

| Coverage | Test source | Command |
|---|---|---|
| Component import boundaries, scene inputs, hit testing/selection, camera bounds, detail links/actions, render notifications/stop/resume | [space-components.test.ts](web/tests/space-components.test.ts) | `bash scripts/docker_test.sh web` |
| Process layout | In-source tests in [process-layout.ts](web/src/space/process-layout.ts) and socket-peer/layout-stability regressions in [process-layout.test.ts](web/tests/process-layout.test.ts) | `bash scripts/docker_test.sh web src/space/process-layout.ts tests/process-layout.test.ts` |
| Data merging, peer/file layout, activity model, resolution adjustment, label placement/fading | In-source tests in [data.ts](web/src/space/data.ts) and [renderer.ts](web/src/space/renderer.ts) | `bash scripts/docker_test.sh web` |
| Persistent Netlink display, classification, shared FDs, exit/state changes, layout | [space-netlink.test.ts](web/tests/space-netlink.test.ts) | `bash scripts/docker_test.sh web` |
| Display labels and UID colors | [space-formatting.test.ts](web/tests/space-formatting.test.ts) | `bash scripts/docker_test.sh web` |
| Observation state/activity paths, SSE stop/resynchronization/stale-notification rejection | [data.ts in-source tests](web/src/space/data.ts) | `bash scripts/docker_test.sh web` |
| WebGL rendering, layout/selection/camera, connection management | [space-browser.ts](web/tests/browser/space-browser.ts), calling [space-netlink.ts](web/tests/browser/space-netlink.ts), [space-network.ts](web/tests/browser/space-network.ts), [space-files.ts](web/tests/browser/space-files.ts) for network/Netlink/files | `bash scripts/docker_test.sh browser` after [browser test preparation](#browser-tests) |
| Live CPU/IPC/file I/O sensors | [space-live.py](tests/space-live.py), [space-files-live.py](tests/space-files-live.py) | See [live sensor tests](#live-sensor-tests) |

## SPACE recording and replay

On live `/space`, choose **Record**, continue observing, then choose **Stop & Download** to download one UTF-8 `.json` file. Recording stays in memory; nothing is uploaded or saved on the server. The initial full snapshot includes maps retained from earlier deltas.

Recordings can contain process arguments, usernames, paths, and IP addresses; review them before sharing. Bundled **Chrome** and **Firefox** demos show real browser activity in Ubuntu 26.04 VMs. Both open the same eight sites in [`scripts/browser_sites.txt`](scripts/browser_sites.txt) and reload every 10 seconds. **Linux kernel build** records 30 seconds of actual clang compilation in a dedicated Ubuntu 26.04 QEMU/KVM guest.

A stream gap, reconnection, hidden tab, or page departure ends recording. The UI shows the reason and does not resume recording across gaps. Retained recordings can be saved with **Download recording** after redisplaying the tab. Navigating away or reloading with an unsaved recording triggers a browser confirmation. Memory is not persistent storage; save before closing the tab. Recording stops at 128 MiB and retains the completed portion. Local replay uses the same size limit; live display continues independently.

**Open recording** opens `/replay/space/`. Select or drop downloaded JSON or a `.json.gz` file compressed with standard gzip. The browser streams it through the File API without uploading. Input and decompressed data each have a 128 MiB limit; invalid gzip or recording data is rejected. Detection uses gzip magic bytes, so data already decompressed through HTTP `Content-Encoding: gzip` is not decompressed twice. Another file can be selected on the same page.

After loading, playback starts automatically with **Loop** enabled, including bundled demos opened by URL. Play/pause, timeline seeking, and speeds of 0.1×, 0.25×, 0.5×, 1×, and 2× share a virtual clock with SPACE activity animations. Rotation/panning/zooming work while paused. Hiding the tab pauses playback. Disable **Loop** to stop at the end; opening another recording re-enables looping. Process-detail links are disabled during replay.

### Recording JSON contract

Version 1 is a single JSON object with these fields:

- `format_version: 1`, `view_type: "space"`, ISO-formatted `started_at`.
- `duration_ms` and `end_reason`.
- `initial_snapshot`: a full `SystemSnapshot`, including complete process maps and FD relations; no earlier SSE messages are needed.
- `events`: chronologically ordered `{ at_ms, type, data }` entries. Times are milliseconds from recording start; equal-time entries preserve array order. `type` is `snapshot` (a full `SystemSnapshot` after merging deltas) or `activity` (`SpaceActivity`).

Payload types are in [api-types.ts](web/src/shared/api-types.ts); the shared runtime parser is in [recording.ts](web/src/space/recording.ts). Unknown versions, other view types, invalid nested values, out-of-order times, and times outside the recording duration are rejected before rendering. Local files and published recordings use the same parser and replay engine.

Playback, seeking, and looping end at the last event time, or zero for snapshot-only recordings. The timeline accepts fractional milliseconds and reaches the exact endpoint at its right edge. `duration_ms` retains actual elapsed recording time, potentially including time between the last event and recording stop. Seeking reconstructs state from the initial snapshot, keeping the format simple but potentially taking time for large recordings.

### Static replay site and Cloudflare Pages size limit

The replay site needs only Node.js 22.12 or later and npm to build:

```sh
npm --prefix web ci
npm --prefix web run build:replay
```

Publish **`web/dist-replay/`** as-is. Output includes `replay/space/index.html`, shared SPACE assets, and bundled `.json`/`.json.gz` recordings, excluding LIST, PROCESS, and LIVE pages. Directory `index.html` files support direct navigation and reloads. `/replay/` is reserved for a future selection page. Replay makes no SSE connections or live API calls.

Choose a recording in the live SPACE or replay menu and press **Open demo** for automatic looping playback. The selector and URL allowlist share `bundledRecordings` in [session-controls.ts](web/src/space/session-controls.ts). When adding recordings to `web/replay-public/recordings/`, also add their IDs, labels, and asset paths there.

Open the Chrome demo at `/replay/space/?recording=chrome-ubuntu26` on the published site. `recording` is an allowlisted ID, not an arbitrary user-supplied fetch URL. Firefox uses `?recording=firefox-ubuntu26`. Embedded and static builds share bundled recordings. Synthetic data exists only in private test fixtures outside published assets.

To meet [Cloudflare Pages Free limits](https://developers.cloudflare.com/pages/platform/limits/), the build fails if any static asset, including a published demo, exceeds 25 MiB. No conversion into a split format or sidecar files is required.

For deployment under a path prefix, build with `PROCINSH_REPLAY_BASE=/prefix/ npm --prefix web run build:replay` and serve output under that prefix. Asset URLs and demo fetches follow the base path.

Regular `npm --prefix web run build` writes `web/dist/` for Rust embedding, providing live `/space` and `/replay/space/` as separate pages.

### Recording browser demos

VM scripts require a Linux x86-64 host with read/write access to `/dev/kvm`, QEMU (`qemu-system-x86`, `qemu-utils`), `cloud-image-utils`, curl, and Python 3. If necessary, add your user to `kvm` with `sudo usermod -aG kvm "$USER"` and log in again.

```sh
# The guest clones the current checkout's published commit.
# WORKLOAD is chrome (default) or firefox.
WORKLOAD=firefox VM_DIR=/tmp/space-demo scripts/recording-vm/run.sh prepare
VM_DIR=/tmp/space-demo scripts/recording-vm/run.sh run
```

The guest installs procinsh and the selected browser, then starts the collector and browser workload. Both browsers use the same Puppeteer WebDriver BiDi calls for tab creation, cache disabling, navigation, and reloads, with fresh profiles and the shared site list in `scripts/browser_sites.txt`.

Wait for `SPACE recording ready on guest port 10090` in `/tmp/space-demo/console.log`, then open `http://127.0.0.1:10090/space` and use **Record**/**Stop & Download**. Host and guest share port 10090. Set `HOST_PORT` during `prepare` to use a different port; `run` preserves it. `VM_CPUS`, `VM_MEMORY_MIB`, and `VM_DISK_GIB` configure VM resources.

To record automatically using the same browser interactions:

```sh
(cd web && WORKLOAD=firefox HOST_PORT=10090 RECORD_SECONDS=15 \
  RECORD_OUTPUT=/tmp/firefox-vm.json.gz node --import tsx scripts/capture-vm-recording.ts)
```

The host needs Google Chrome and `npm --prefix web ci`. `CHROME` sets the executable path. The capture script saves gzip and verifies activity, file size, and replay. Compress manual downloads with `gzip -n -9 recording.json`. Copy demos to `web/replay-public/recordings/` and add entries in `web/src/space/session-controls.ts`.

Connect to the guest console with `VM_DIR=/tmp/space-demo python3 scripts/recording-vm/console.py` (Ctrl-] disconnects). Inspect the workload with `journalctl -u browser-demo`. The `browser-demo` systemd service accepts `TAB_COUNT`, `INTERVAL_SECONDS`, and `SITE_LIST`. Stop the VM with `VM_DIR=/tmp/space-demo scripts/recording-vm/run.sh stop` and wait for QEMU to exit before restarting.

### Recording the Linux kernel build demo

The kernel workload uses the same VM tools and browser recording features as browser workloads. `WORKLOAD` accepts `chrome` (default), `firefox`, or `kernel-build`. Kernel guests neither install browsers nor run browser workloads.

```sh
WORKLOAD=kernel-build VM_DIR=/tmp/kernel-demo HOST_PORT=10191 \
  scripts/recording-vm/run.sh prepare
VM_DIR=/tmp/kernel-demo scripts/recording-vm/run.sh run
# Wait for the ready message in /tmp/kernel-demo/console.log.
(cd web && WORKLOAD=kernel-build HOST_PORT=10191 \
  RECORD_OUTPUT=/tmp/linux-build-ubuntu26.json.gz \
  node --import tsx scripts/capture-vm-recording.ts)
```

The guest shallow-clones pinned tag `v7.0` of `torvalds/linux`. Following the [reference article](https://akawashiro.com/articles/linux-build-script), it sets `CC=clang`, applies `defconfig` then `kvm_guest.config`, and compiles with separate source/output trees and `max(1, nproc - 2)` parallel jobs. Source is in `/home/demo/linux`, output in `/home/demo/linux-build`. The guest kernel version does not select or override the build target. The built kernel is neither installed nor used to reboot the guest.

Defaults are four vCPUs, 8,192 MiB RAM, and a 40 GiB disk. The workload service starts after downloading/configuring source. Inspect the build from the serial console with `journalctl -u kernel-build-demo` or `/home/demo/kernel-build.log`.

The capture command waits for running clang processes and positive compiler CPU deltas in the live stream, then records 30 seconds by default. It verifies compiler CPU/file activity across all snapshots in the recording. Interrupted recordings are rejected; gzip replay is verified before saving. `RECORD_SECONDS` changes duration. Browser workloads default to 15 seconds.

Record during the initial build. For another clean recording, prepare a new VM with empty output. If JSON exceeds 128 MiB or gzip exceeds 25 MiB, reduce `VM_CPUS` in a fresh guest and record another 30 seconds.

Copy verified gzip to `web/replay-public/recordings/`. Bundled demo ID `linux-build-ubuntu26` is available through the demo menu, static site's landing page, and `/replay/space/?recording=linux-build-ubuntu26`. After capture, stop the disposable VM with `VM_DIR=/tmp/kernel-demo scripts/recording-vm/run.sh stop`.

The bundled recording compiles upstream Linux `v7.0` in an Ubuntu 26.04 guest with Ubuntu clang 21.1.8 and two parallel jobs. It contains 50 compiler identities with real CPU/file activity; the gzip asset is about 4 MiB.

### Recording/replay verification

Run `bash scripts/docker_test.sh ordinary`, `bash scripts/docker_test.sh checks`, and `bash scripts/docker_test.sh browser`.

## Permissions

`process_vm_readv` is constrained by ownership, dumpable attributes, Yama, `CAP_SYS_PTRACE`, seccomp, and other restrictions. BPF additionally requires kernel support and observation permissions. The application does not automatically change permissions/kernel settings or run sudo.

The default listen address is `127.0.0.1:8080`. IPv4/IPv6 loopback addresses such as `127.0.0.1` and `::1` require no extra flag. Non-loopback addresses, including LAN addresses and wildcards (`0.0.0.0`, `::`), cause a nonzero exit before binding unless `--allow-non-loopback` is specified. For example: `procinsh --listen 0.0.0.0:9090 --allow-non-loopback`. Explicitly allowed non-loopback listening still produces a warning.

Repeating `--listen` makes one process listen on multiple addresses while sharing collection. Explicit addresses replace the default. All addresses are validated/bound before serving; any failure exits nonzero. Each listener validates Host against its own address. SIGINT/SIGTERM stops all listeners and collection. For example, use the same port on Tailscale and localhost with:

```sh
./scripts/dev_run.sh --listen "$(tailscale ip -4):9090" \
  --listen 127.0.0.1:9090 --listen '[::1]:9090' --allow-non-loopback
```

There is no authentication or TLS. Anyone who can connect can access process memory and environment variables; restrict non-loopback listening to trusted networks with controlled access. Host/Origin/Fetch Metadata validation and API `Cache-Control: no-store` remain in effect, but do not replace authentication.

## Logging

Logging uses `log` and `env_logger`, writing timestamps, levels, source file paths, and line numbers (for example, `src/http_server/system/service.rs:123`) to stderr. Default level is `info`. `RUST_LOG` filters still use module names.

- `info`: startup/shutdown, observation start/stop, target exit, collection state/recovery.
- `warn`: observation failures, unavailable sensors; repeated identical states/errors are suppressed.
- `error`: fatal execution failures, worker failures, HTTP 5xx.
- `debug`: HTTP method/path/status/response-generation time, API error details, structure collection counts, SSE event emission. SSE logs API path/event name per connection; `/api/system/events` also logs `payload_bytes` (UTF-8 JSON body bytes, excluding SSE line framing and HTTP headers). Process observations include identity; delivery gaps include dropped counts. Payloads and keep-alives are not logged. Emission logs mean the server handed an event to the stream, not that a client finished receiving it.

```sh
RUST_LOG=debug ./scripts/dev_run.sh --listen 127.0.0.1:9090
RUST_LOG=info,procinsh_privileged_helper::http_server::system=debug ./scripts/dev_run.sh --listen 127.0.0.1:9090
```

To filter only HTTP access logs, use `RUST_LOG=info,procinsh::http_server::middleware=debug`. Existing subsystem filters also apply to child modules.

`RUST_LOG=off` suppresses application logs. Access logs exclude queries, tokens, bodies, observed memory, and environment values. SSE response timing measures only the initial connection response.

## Verification

The host needs Linux x86-64, Docker Engine, and Docker daemon access from a regular account. Tests, test builds, and fixture preparation all run inside containers. Host-side Rust, Node.js, sudo, and setcap are unnecessary.

```sh
# Ordinary tests, formatting/static analysis, kernel/live sensor verification
bash scripts/docker_test.sh

# Individual suites
bash scripts/docker_test.sh ordinary
bash scripts/docker_test.sh checks
bash scripts/docker_test.sh kernel
bash scripts/docker_test.sh browser

# Select distribution and tests
bash scripts/docker_test.sh --distro fedora:44 ordinary
bash scripts/docker_test.sh rust live_threads_churn_sleep_and_exit --nocapture
bash scripts/docker_test.sh web src/space/process-layout.ts
```

On hosts restricting Docker bridge DNS or outbound traffic, `bash scripts/docker_test.sh --network host` uses host networking for builds and execution. The default is bridge networking without exposing test ports on the host.

`ordinary` builds all targets and runs every Rust test, Vitest, listen policy tests, and Vite development-server integration tests in containers with required capabilities. Rust tests do not use `#[ignore]`; normal `cargo test --locked` also runs perf, pidfd, and network namespace tests. `checks` verifies Rust/C/TypeScript formatting, Clippy, rustdoc, TypeDoc, TypeScript dependency graphs, and crate packaging. Every suite installs web dependencies, checks types, and builds.

`kernel` runs live CPU/IPC, file I/O, and signal eBPF tests. `rust` passes filters and test arguments such as `--nocapture` to `cargo test --locked -- <arguments>`. Rust tests always require an environment with observation permissions; missing permissions or features cause failure.

The kernel needs BTF (`CONFIG_DEBUG_INFO_BTF=y` and `/sys/kernel/btf/vmlinux`), eBPF, perf, ptrace, network namespaces, pidfd_getfd, and the application's BPF hooks. Docker Desktop VM kernels, rootless Docker, user namespace remapping, and daemons with strong LSM restrictions may not provide the required access. `ordinary`, `kernel`, `rust`, and `browser` explicitly add `BPF`, `PERFMON`, `SYS_PTRACE`, `DAC_READ_SEARCH`, and `SYS_ADMIN`. Only `kernel` and `browser`, which verify live eBPF, use `--pid=host` to align BPF event PIDs with procfs PIDs. Every suite mounts kernel BTF read-only and retains Docker's default seccomp and AppArmor. Neither `--privileged` nor unrestricted profiles are used.

The checkout is mounted read-only and source copied to `/work` inside the container. Fixtures, web output, and HMR edits operate on that copy. Rust build caches use per-distro Docker volumes named `procinsh-test-target-*`; dependencies use `procinsh-test-registry`. No root-owned output is written to the checkout. Suites for the same distro share build caches and must run sequentially. Caches survive container exit; remove them with `docker volume rm <volume-name>` when no longer needed.

TypeScript sources, tests, and configuration (`web/src/**/*.ts`, `web/tests/**/*.ts`, `web/*.ts`) use [Prettier](https://prettier.io/docs/install). The version is pinned in [package.json](web/package.json); [.prettierrc.json](web/.prettierrc.json) specifies standard formatting and LF line endings. [.prettierignore](web/.prettierignore) excludes generated files and node_modules. Local development and CI use the same check commands.

```sh
npm --prefix web run format
npm --prefix web run format:check
```

`format` rewrites files; `format:check` makes no changes and fails on formatting differences. Format TypeScript changes before committing.

C sources/headers, including BPF and test programs, follow `.clang-format`. clang-format is pinned to **21.1.8** in `requirements-format.txt`; local development and CI use the same `scripts/format_c.py`. The script checks the executable version and fails on mismatch.

```sh
python3 -m venv .venv-format
.venv-format/bin/python -m pip install -r requirements-format.txt
CLANG_FORMAT="$PWD/.venv-format/bin/clang-format" python3 scripts/format_c.py
CLANG_FORMAT="$PWD/.venv-format/bin/clang-format" python3 scripts/format_c.py --check
```

`--check` does not rewrite files and fails on formatting differences. It checks tracked and untracked `.c`/`.h` files, excluding ignored generated files such as `vmlinux.h`. Format C changes before committing. Updating clang-format combines changing the pinned version with reformatting all C files.

CI runs Rust/C/TypeScript formatting checks, TypeScript type checking/Vite builds, Vitest, crate asset inclusion and builds without Node, all-target Rust builds, Clippy, rustdoc link validation, Rust tests, log checks, list/detail data and history graphs, and SPACE model/data verification. Live sensors run in the kernel job; browser tests run separately through Docker's browser suite.

The [Publish documentation and replay demo](.github/workflows/docs.yml) workflow runs on pushes to `main` and manual dispatch, generating rustdoc, TypeDoc, and Rust/TypeScript module dependency graphs and publishing `target/doc/` as a single GitHub Pages artifact. Rust docs use `/procinsh/`, TypeScript `/typescript/`, Rust graphs `/rust-architecture/`, and TypeScript graphs `/typescript-architecture/`, all linked from the landing page. PR `CI` checks rustdoc links and TypeDoc/TypeScript graph generation. Rust graph generation and `cargo-modules` installation run in the publishing workflow.

The same Pages artifact includes a browser-only [SPACE replay demo](https://akawashiro.github.io/procinsh/demo/) under `/demo/` for Chrome, Firefox, and Linux kernel build recordings. The publishing workflow runs `npm --prefix web run build:replay` and copies `web/dist-replay/` to `target/doc/demo/`. The `/demo/` entry page links to automatic looping playback of each recording. Documentation and demos publish together in one workflow; the PR `Static SPACE replay build` workflow checks only the build. JS/CSS and recording references use relative URLs, so `web/dist-replay/` can deploy to any path such as `/software/procinsh/` without rebuilding. Use an entry URL ending in `/` or specifying `index.html`, and copy all output preserving directory structure. Browser tests place the same output under `/`, `/procinsh/demo/`, and `/software/procinsh/` in turn and verify all recordings. To publish, merge the PR to `main` and check demo links after the publishing workflow succeeds. For manual republication, run that workflow against `main`.

Vitest runs unit tests in `web/tests/` and in-source tests inside `if (import.meta.vitest)` in `web/src/`. In-source tests access internal helpers directly rather than exporting solely for tests. `includeSource` in `vitest.config.ts` discovers these tests; regular Vite builds replace `import.meta.vitest` with `undefined`, removing tests and test imports. Tests normally use Node; only DOM component tests use jsdom. Unit, browser, development-server tests, and shared helpers all fall under `npm --prefix web run typecheck`.

TypeScript docs use [typedoc.json](web/typedoc.json) to recursively expand `web/src/` as entry points. They cover exported types/functions in list, details, SPACE, and shared modules, excluding external library docs. Locally, run `npm --prefix web ci` then `npm --prefix web run docs` and open `target/doc/typescript/index.html`.

[module-dependencies.ts](web/scripts/module-dependencies.ts) uses the TypeScript compiler API to analyze imports/re-exports in `web/src/` and draws SVG dependency graphs with Graphviz. This includes type-only imports, `import("...")` type references, and string-literal dynamic imports. External packages, `web/tests/`, and test blocks inside `if (import.meta.vitest)` are excluded. Arrows point from consumers to dependencies; the overview groups by directories directly under the source root, and detail diagrams show files.

Install Graphviz's `dot` locally and run:

```sh
npm --prefix web run docs:architecture
```

Open generated output at `target/doc/typescript-architecture/index.html`.

CI uses the same `bash scripts/docker_test.sh` entry point. An Ubuntu 24.04, Ubuntu 26.04, and Fedora 44 matrix runs `ordinary` with `fail-fast: false` to show each result. Separate Ubuntu 24.04 jobs run `checks` and `kernel`. Distro containers share the runner's kernel/BTF and verify userspace differences, not distribution-specific kernels.

Rust tests cover identities, SSE, procfs, PID reuse, memory reading, perf samples, symbols, HTTP, system-wide structure, and aggregation. Live perf fixtures verify known R12–R15 values, recursion/stack size limits, initial/periodic ptrace while sleeping and transition to perf, PIE/non-PIE/shared libraries with omitted frame pointers, multiple TIDs, churn, and target exit.

### Browser tests

```sh
bash scripts/docker_test.sh browser
```

Docker images used by every suite include Google Chrome. `browser` verifies both list/details and SPACE inside containers. Browsers use temporary profiles and are cleaned up with test servers on exit. SPACE builds the test web fixture through [vite.browser-test.config.ts](web/vite.browser-test.config.ts), embeds it in the Cargo binary, then runs it. Generated files stay inside the container.

[dev-server.ts](web/tests/integration/dev-server.ts) is part of `ordinary`. It starts the built binary and Vite and checks page routes, CSS-edit HMR, Origin validation, and proxying of JSON and both SSE streams.

### Generating communication/IPC load with Chrome

`scripts/chrome_load.mjs` reloads Google Chrome tabs about every 10 seconds, bypassing cache. It requires Node.js 22.12 or later and Chrome, with no additional npm packages. By default, it opens the first 20 sites in `scripts/chrome_sites.txt`. GPU is disabled; without a graphical session (`DISPLAY`/`WAYLAND_DISPLAY`), it runs headless.

```sh
node scripts/chrome_load.mjs

# 100 sites, 10-second interval
TAB_COUNT=100 INTERVAL_SECONDS=10 node scripts/chrome_load.mjs

# Specify Chrome executable and URL to observe
CHROME=/usr/bin/chromium node scripts/chrome_load.mjs http://127.0.0.1:8080/

# Custom site list (one URL per line; ignore blank lines and lines starting with #)
SITE_LIST=/path/to/sites.txt TAB_COUNT=5 node scripts/chrome_load.mjs
```

`TAB_COUNT` ranges from 1 to 1,000; `INTERVAL_SECONDS` must be at least one second. The list repeats if there are more tabs than URLs. Loading times, redirects, and access restrictions vary by site; logged reload completion means a DevTools command succeeded, not that all content finished loading.

Chrome uses a separate temporary profile and loopback DevTools port; controller/Chrome PIDs are logged. Ctrl+C or SIGTERM to the controller stops Chrome and removes the profile. `PID_FILE=/path/to/load.pid` saves the controller PID and removes it on exit. Existing PID files are not overwritten.

Use tmux to keep running after an interactive session ends. Run from the repository root:

```sh
tmux new-session -d -s procinsh-load \
  "exec env PID_FILE=/tmp/procinsh-chrome-load.pid node '$PWD/scripts/chrome_load.mjs' >> /tmp/procinsh-chrome-load.log 2>&1"
tail -f /tmp/procinsh-chrome-load.log

# Stop load generation
kill -TERM "$(cat /tmp/procinsh-chrome-load.pid)"
```

### Live sensor tests

```sh
bash scripts/docker_test.sh kernel
```

Inside the container, a temporary server and sensors start before fixtures are created. [space-live.py](tests/space-live.py), [space-files-live.py](tests/space-files-live.py), and [space-signals-live.py](tests/space-signals-live.py) verify CPU/IPC, file I/O, and signals once observation is ready. Unavailable sensors cause failure; the server stops and logs are printed on both success and failure.

C fixtures in `tests/targets/` cover computation, sleep, thread churn, memory allocation, recursion, mmap, IPC, and activity measurement. For manual observation:

```sh
tests/targets/bin/recursive --allow-inspector
```

`--allow-inspector` is a test-only option granting ptrace access to that fixture. It does not alter system-wide Yama settings.

## Related links

- [Rust module dependencies](https://akawashiro.github.io/procinsh/rust-architecture/)
- [TypeScript module dependencies](https://akawashiro.github.io/procinsh/typescript-architecture/)
- [Rust doc](https://akawashiro.github.io/procinsh/procinsh/index.html)
