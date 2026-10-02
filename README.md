# ProcInSh - Process in the Shell

A web-based process inspector for Linux.
Like [Ghost in the Shell](https://en.wikipedia.org/wiki/Ghost_in_the_Shell), you can wander through process space with your ghost.

## Screenshot

<img src="./images/procinsh_movie.gif" alt="Demo of procinsh" width="800">

<img src="./images/procinsh_top.png" alt="Screenshot of space view" width="800">

<img src="./images/procinsh_procinsh.png" alt="Screenshot of a process in 3D" width="800">

<img src="./images/procinsh_tmux.png" alt="Screenshot of process details" width="800">

<img src="./images/procinsh_list.png" alt="Screenshot of the process list" width="800">

## Usage

Requires Linux x86-64. Both installation methods compile native code and require
Rust, a C compiler, clang with the BPF backend, bpftool, pkg-config, libelf and
zlib development files, autoconf/automake, autopoint, flex, bison, gawk, and BTF information at `/sys/kernel/btf/vmlinux`.

### Install from crates.io

Install [procinsh from crates.io](https://crates.io/crates/procinsh) and run it:

(Sorry, we assume you are using Ubuntu, please reinterpret not so)
```sh
sudo apt-get install --yes --no-install-recommends \
         build-essential clang llvm pkg-config libelf-dev zlib1g-dev python3 \
         autoconf automake autopoint flex bison gawk \
         linux-tools-common linux-tools-generic
cargo install procinsh --locked
```

Then run and open http://127.0.0.1:9090 in your browser. To allow remote
access, use `--allow-non-loopback`, but be careful: this exposes process memory
and environment variables without any authentication.

```
sudo "$HOME/.cargo/bin/procinsh" --listen 127.0.0.1:9090
```

If you don't want to use `sudo`, please use setcap instead of it.

```
sudo setcap \
  cap_sys_ptrace,cap_bpf,cap_perfmon=ep \
  "$HOME/.cargo/bin/procinsh"
"$HOME/.cargo/bin/procinsh" --listen 127.0.0.1:9090
```

### Self build

Also requires Node.js 22 or newer with npm and Rust via rustup. The Rust version
and components are pinned in `rust-toolchain.toml`; rustup installs them
automatically when needed.

Clone this repository and run the following from its root:

```sh
sudo apt-get install --yes --no-install-recommends \
         build-essential clang llvm pkg-config libelf-dev zlib1g-dev python3 \
         autoconf automake autopoint flex bison gawk \
         linux-tools-common linux-tools-generic
npm ci
npm run build:web
cargo build --release --locked
```

Then run:

```sh
sudo ./target/x86_64-unknown-linux-gnu/release/procinsh --listen 127.0.0.1:9090
```

### Building on WSL2 (Ubuntu)

Ubuntu's `bpftool` wrapper may fail with `bpftool not found for kernel ...`
because the WSL2 kernel version differs from the Ubuntu tools package. Set
`BPFTOOL` to the packaged executable directly, bypassing the wrapper:

```sh
sudo apt-get update
sudo apt-get install --yes --no-install-recommends \
         build-essential clang llvm pkg-config libelf-dev zlib1g-dev python3 \
         autoconf automake autopoint flex bison gawk \
         linux-tools-common linux-tools-generic
for tool in /usr/lib/linux-tools/*/bpftool; do
    if [ -x "$tool" ]; then
        export BPFTOOL="$tool"
        break
    fi
done
"${BPFTOOL:?No packaged bpftool found; install linux-tools-generic}" version
"$BPFTOOL" btf dump file /sys/kernel/btf/vmlinux format c >/tmp/procinsh-vmlinux.h
npm ci
npm run build:web
cargo build --release --locked
```

Then run:

```sh
sudo ./target/x86_64-unknown-linux-gnu/release/procinsh --listen 127.0.0.1:9090
```

### Static binary and containers

ProcInSh observes its own PID namespace: on the host it observes host processes;
inside a container it observes processes visible in that container, including
nested PID namespaces. Use the container's normal `/proc` mount. Neither a
sidecar nor `--pid=host` is needed.

Repository builds use GNU/glibc static linking by default, configured in
`.cargo/config.toml`. This applies to debug and release builds. After building
the web assets above:

```sh
cargo build --release --locked
ldd target/x86_64-unknown-linux-gnu/release/procinsh
# statically linked (or: not a dynamic executable)
```

The default build target keeps static flags off host build scripts and proc-macros.
`RUSTFLAGS` overrides Cargo config flags; preserve `-C target-feature=+crt-static`
when adding custom flags. The repository config does not control builds made by
`cargo install procinsh` from crates.io; use
`RUSTFLAGS='-C target-feature=+crt-static' cargo install procinsh --locked --target x86_64-unknown-linux-gnu`
for a static installation.

libbpf, libelf and zlib are vendored and statically linked. The executable also
embeds BPF objects and web assets. The application image only needs to copy the
binary; Rust, clang, bpftool and libbpf/libelf/zlib packages are build dependencies,
not runtime requirements:

```dockerfile
FROM your-application-image
COPY target/x86_64-unknown-linux-gnu/release/procinsh /usr/local/bin/procinsh
```

Run the application and ProcInSh in the same container. For example, start the
application using the image's usual command, then start ProcInSh using `docker exec`:

```sh
docker run -d --name inspected-app \
  --cap-add=SYS_PTRACE --cap-add=BPF --cap-add=PERFMON \
  -p 127.0.0.1:9090:9090 \
  -v /sys/kernel/btf:/sys/kernel/btf:ro \
  image-with-procinsh
docker exec --user 0 -d inspected-app \
  procinsh --listen 0.0.0.0:9090 --allow-non-loopback
```

Open http://127.0.0.1:9090. `SYS_PTRACE` permits process inspection; `BPF` and
`PERFMON` permit activity sensors. `--privileged` is not required. The host kernel
must provide compatible BTF and tracing hooks. The read-only BTF mount makes that
information available when Docker hides `/sys/kernel`; it contains kernel type
information, not the host's `/proc`. Host security policy, seccomp, or user namespace
restrictions may still deny BPF/ptrace; sensor status reports the failure.

User names come from the container's `/etc/passwd`; group names are not resolved.
Reverse DNS uses glibc and the container's `/etc/hosts`, `/etc/nsswitch.conf` and
`/etc/resolv.conf`. Standard files/DNS lookup is tested; custom NSS modules are
not bundled with the static binary.

The integration test checks static linkage, procfs/CPU/IPC/file PID agreement,
exclusion of a host-only process, user names and reverse lookup:

```sh
python3 tests/container-integration.py
```

It requires Docker access and a BPF-capable host kernel. Python in the test image
is only the test driver; ProcInSh itself has no Python runtime dependency.

_Now, where shall I go? The process space is vast._
