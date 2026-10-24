# Privileged helper boundary

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

## Ownership inventory

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

## Launch, protocol and lifetime

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

## Installation, upgrades and containers

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

## Architectural guard and verification

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
