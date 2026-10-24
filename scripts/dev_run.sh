#!/bin/bash
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
helper="$DIR/target/debug/procinsh-privileged-helper"
if [[ -z $(getcap "$helper") ]]; then
  sudo -n setcap \
    cap_sys_ptrace,cap_bpf,cap_perfmon,cap_dac_read_search,cap_sys_admin=ep \
    "$helper"
fi
exec "$DIR/target/debug/procinsh" "$@"
