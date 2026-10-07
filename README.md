# ProcInSh - Process in the Shell

A web-based process inspector for Linux.
Like [Ghost in the Shell](https://en.wikipedia.org/wiki/Ghost_in_the_Shell), you can wander through process space with your ghost.

See also [a blog post](https://akawashiro.com/articles/procinsh-en).

## Screenshot

<img src="./images/procinsh_movie.gif" alt="Demo of procinsh" width="800">

<img src="./images/procinsh_top.png" alt="Screenshot of space view" width="800">

<img src="./images/procinsh_procinsh.png" alt="Screenshot of a process in 3D" width="800">

<img src="./images/procinsh_tmux.png" alt="Screenshot of process details" width="800">

<img src="./images/procinsh_list.png" alt="Screenshot of the process list" width="800">

## Usage

Requires Linux x86-64. Both installation methods compile native code and require
Rust via rustup, a native linker, the pinned BPF Rust toolchain and bpf-linker,
and BTF information at `/sys/kernel/btf/vmlinux`. BPF sensors are written in Rust
with Aya and compiled for the kernel used during installation. After a kernel
update, reinstall/rebuild: sensors are disabled when the running kernel BTF
differs from the build BTF. See [BPF setup](docs/DEVELOPMENT.md#bpf-toolchain).

Support on WSL2 is very limited, I recommend building procinsh from source rather than installing it from crates.io.
Container builds are tested; runtime observation in containers is not yet tested.

### Install from crates.io

Install [procinsh from crates.io](https://crates.io/crates/procinsh) and run it:

(Sorry, we assume you are using Ubuntu, please reinterpret not so)
```sh
sudo apt-get install --yes --no-install-recommends \
         build-essential gh zstd
# Prepare the pinned BPF toolchain/linker using the instructions linked above.
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
  cap_sys_ptrace,cap_bpf,cap_perfmon,cap_dac_read_search=ep \
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
         build-essential gh zstd
./scripts/setup_bpf.sh
export PATH="$PWD/target/bpf-tools:$PATH"
npm ci
npm run build:web
cargo build --release --locked
```

Then run:

```sh
sudo ./target/release/procinsh --listen 127.0.0.1:9090
```

### Building on WSL2 (Ubuntu)

The build and execution kernel must expose `/sys/kernel/btf/vmlinux` and the
required BPF tracing hooks. Use the same pinned BPF tools as on native Linux;
no kernel-version-specific bpftool package is needed. A kernel without BTF
cannot build the sensors.
Then run:

```sh
sudo ./target/release/procinsh --listen 127.0.0.1:9090
```

_Now, where shall I go? The process space is vast._
