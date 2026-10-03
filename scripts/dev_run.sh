#!/bin/bash
set -euo pipefail

sudo -n setcap \
  cap_sys_ptrace,cap_bpf,cap_perfmon,cap_dac_read_search=ep \
  /home/akira/ghq/github.com/akawashiro/procinsh/target/debug/procinsh
exec target/debug/procinsh "$@"
