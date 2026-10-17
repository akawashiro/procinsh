#!/bin/bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
profile=debug
if [[ ${1:-} == --release ]]; then
  profile=release
  shift
fi
sudo -n setcap \
  cap_sys_ptrace,cap_bpf,cap_perfmon,cap_dac_read_search,cap_sys_admin=ep \
  "$DIR/target/$profile/procinsh" 2>/dev/null || true
exec "$DIR/target/$profile/procinsh" "$@"
