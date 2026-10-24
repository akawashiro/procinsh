# ProcInSh - Process in the Shell

ProcInSh (pronounced "proc-in-shell") is a web-based process inspector for Linux.
Like [Ghost in the Shell](https://en.wikipedia.org/wiki/Ghost_in_the_Shell), you can wander through process space with your ghost.

[Interactive demo](https://akawashiro.github.io/procinsh/demo/replay/space/?recording=chrome-ubuntu26&tour=1), [Blog post](https://akawashiro.com/articles/procinsh-en), [Discord](https://discord.gg/aQuNSczpw)

## Screenshot

<img src="./images/procinsh_movie.gif" alt="Demo of procinsh" width="800">

<img src="./images/procinsh_space.png" alt="Screenshot of space view" width="800">

<img src="./images/procinsh_space_chrome.png" alt="Screenshot of a process in 3D" width="800">

<img src="./images/procinsh_process_chrome.png" alt="Screenshot of process details" width="800">

<img src="./images/procinsh_list.png" alt="Screenshot of the process list" width="800">

## Usage

Requires Linux x86-64. Both installation methods compile native code and require
Rust, a C compiler, clang with the BPF backend, bpftool, pkg-config, libelf and
zlib development files, and BTF information at `/sys/kernel/btf/vmlinux`.

Support on WSL2 is very limited, I recommend building procinsh from source rather than installing it from crates.io.

### Install from crates.io

Install [procinsh from crates.io](https://crates.io/crates/procinsh) and run it:

(Sorry, we assume you are using Ubuntu, please reinterpret not so)
```sh
sudo apt-get install --yes --no-install-recommends \
         build-essential clang llvm pkg-config libelf-dev zlib1g-dev libcap2-bin python3 \
         linux-tools-common linux-tools-generic
cargo install procinsh --locked
```

Cargo installs two executables. Place both in a root-owned directory and grant
inspection capabilities **only to the helper**:

```sh
sudo install -d -o root -g root -m 0755 /usr/local/libexec/procinsh/installed
sudo install -o root -g root -m 0755 "$HOME/.cargo/bin/procinsh" \
  "$HOME/.cargo/bin/procinsh-privileged-helper" /usr/local/libexec/procinsh/installed/
sudo setcap \
  cap_sys_ptrace,cap_bpf,cap_perfmon,cap_dac_read_search,cap_sys_admin=ep \
  /usr/local/libexec/procinsh/installed/procinsh-privileged-helper
/usr/local/libexec/procinsh/installed/procinsh --listen 127.0.0.1:9090
```

Open http://127.0.0.1:9090. The HTTP process runs without elevated capabilities;
its sibling helper performs inspection over private IPC. Keep the helper and
all its parent directories root-owned and unwritable by ordinary users. Never
apply capabilities in `~/.cargo/bin` or to `procinsh`. You may omit capabilities
to reduce inspection coverage; unavailable operations report errors.

Remote access requires `--allow-non-loopback` and exposes process memory and
environment variables without authentication. See the
[deployment guide](DEVELOPMENT.md#privileged-helper-boundary) for upgrades, containers
and helper configuration.

### Self build on Linux

#### Ubuntu

Also requires Node.js 22.12 or newer with npm and Rust via rustup. The Rust version
and components are pinned in `rust-toolchain.toml`; rustup installs them
automatically when needed.

Clone this repository and run the following from its root:

```sh
sudo apt-get install --yes --no-install-recommends \
         build-essential clang llvm pkg-config libelf-dev zlib1g-dev libcap2-bin python3 \
         linux-tools-common linux-tools-generic
npm --prefix web ci
npm --prefix web run build
cargo build --release --locked
```

Then run:

```sh
helper=$(sudo bash scripts/install_helper.sh target/release/procinsh-privileged-helper)
"$(dirname "$helper")/procinsh" --listen 127.0.0.1:9090
```

#### NixOS

With Rust installed via rustup, create `shell.nix` in the repository root:

```nix
{ pkgs ? import <nixpkgs> { } }:
pkgs.mkShell {
  packages = with pkgs; [ nodejs_22 clang llvm pkg-config elfutils zlib libcap bpftools gnumake ];
  hardeningDisable = [ "all" ];
}
```

Build the web assets and binary inside the Nix shell:

```sh
nix-shell --run 'npm --prefix web ci --no-audit --no-fund && npm --prefix web run build && cargo build --release --locked'
```

The binary links to libraries such as `libelf` in `/nix/store`. Create a GC root
to keep these dependencies from being removed by `nix-collect-garbage`:

```sh
nix-build shell.nix -A inputDerivation -o .nix-gcroot
```

Keep `.nix-gcroot` while using the binary. Then run:

```sh
helper=$(sudo bash scripts/install_helper.sh target/release/procinsh-privileged-helper)
"$(dirname "$helper")/procinsh" --listen 127.0.0.1:9090
```

### Building on WSL2 (Ubuntu)

Ubuntu's `bpftool` wrapper may fail with `bpftool not found for kernel ...`
because the WSL2 kernel version differs from the Ubuntu tools package. Set
`BPFTOOL` to the packaged executable directly, bypassing the wrapper:

```sh
sudo apt-get update
sudo apt-get install --yes --no-install-recommends \
         build-essential clang llvm pkg-config libelf-dev zlib1g-dev libcap2-bin python3 \
         linux-tools-common linux-tools-generic
for tool in /usr/lib/linux-tools/*/bpftool; do
    if [ -x "$tool" ]; then
        export BPFTOOL="$tool"
        break
    fi
done
"${BPFTOOL:?No packaged bpftool found; install linux-tools-generic}" version
"$BPFTOOL" btf dump file /sys/kernel/btf/vmlinux format c >/tmp/procinsh-vmlinux.h
npm --prefix web ci
npm --prefix web run build
cargo build --release --locked
```

Then run:

```sh
helper=$(sudo bash scripts/install_helper.sh target/release/procinsh-privileged-helper)
"$(dirname "$helper")/procinsh" --listen 127.0.0.1:9090
```

_Now, where shall I go? The process space is vast._
