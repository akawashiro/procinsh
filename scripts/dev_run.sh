#!/bin/bash
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
revision=$(sha256sum "$DIR/target/debug/procinsh" "$DIR/target/debug/procinsh-privileged-helper" | cut -d ' ' -f 1 | sha256sum | cut -d ' ' -f 1)
export PROCINSH_HELPER="/usr/local/libexec/procinsh/$revision/procinsh-privileged-helper"
# Reuse an immutable protected pair when already provisioned.
if [[ ! -x $PROCINSH_HELPER ]]; then
  PROCINSH_HELPER=$(sudo -n bash "$DIR/scripts/install_helper.sh" "$DIR/target/debug/procinsh-privileged-helper")
fi
sudo -n setcap -r "$DIR/target/debug/procinsh" 2>/dev/null || true
exec "$(dirname "$PROCINSH_HELPER")/procinsh" "$@"
