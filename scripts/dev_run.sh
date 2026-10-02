#!/bin/bash
set -euo pipefail

binary=$(realpath "${PROCINSH_BINARY:-target/debug/procinsh}")
if [[ $(id -u) != 0 ]]; then
  sudo -n setcap cap_sys_ptrace,cap_bpf,cap_perfmon=ep "$binary"
fi
exec "$binary" "$@"
