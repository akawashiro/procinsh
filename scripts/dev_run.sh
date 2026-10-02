#!/bin/bash
set -euo pipefail

sudo setcap \
  cap_sys_ptrace,cap_bpf,cap_perfmon=ep \
  target/debug/procinsh
exec target/debug/procinsh "$@"
