#!/bin/bash
# Install the pinned BPF tools. Normal Cargo builds only check/use these tools.
set -euo pipefail
cd "$(dirname "$0")/.."
config=bpf-toolchain.toml
setting() { sed -n "s/^$1 = \"\(.*\)\"$/\1/p" "$config"; }
toolchain=$(setting channel)
linker_version=$(setting linker_version)
archive_sha256=$(setting archive_sha256)
tools_dir=${1:-"$PWD/target/bpf-tools"}
mkdir -p "$tools_dir"
tools_dir=$(cd "$tools_dir" && pwd)
rustup toolchain install "$toolchain" --profile minimal --component rust-src --component clippy
archive=bpf-linker-x86_64-unknown-linux-musl.tar.zst
gh release download "v$linker_version" --repo aya-rs/bpf-linker \
  --pattern "$archive" --dir "$tools_dir" --clobber
printf '%s  %s\n' "$archive_sha256" "$tools_dir/$archive" | sha256sum --check
tar --zstd -xf "$tools_dir/$archive" -C "$tools_dir"
"$tools_dir/bpf-linker" --version
printf 'Add %s to PATH before running Cargo.\n' "$tools_dir"
