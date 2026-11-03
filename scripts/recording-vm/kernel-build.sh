#!/bin/bash

# Build upstream Linux v7.0 in the guest with separate source and output.
set -euo pipefail
source_dir=/home/demo/linux
build_dir=/home/demo/linux-build
cpus=$(nproc)
jobs=$((cpus > 2 ? cpus - 2 : 1))
kernel_make() {
  make --no-print-directory -C "$source_dir" O="$build_dir" CC=clang "$@"
}
case ${1:-} in
  prepare)
    git clone --depth 1 --branch v7.0 https://github.com/torvalds/linux "$source_dir"
    mkdir -p "$build_dir"
    kernel_make defconfig
    kernel_make kvm_guest.config
    ;;
  build)
    kernel_make -j "$jobs"
    ;;
  *) echo "Usage: $0 {prepare|build}" >&2; exit 2 ;;
esac
