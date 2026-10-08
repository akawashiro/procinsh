#!/bin/bash
set -euo pipefail

case "${1:-}" in
  address) [[ $# == 1 ]] ;;
  run) [[ $# == 3 ]] ;;
  *) echo "Usage: $0 {address|run BINARY PORT}" >&2; exit 2 ;;
esac

# Resolve the local node's IPv4 address on each start. Never fall back to a
# wildcard or LAN address when Tailscale is unavailable.
address=$(tailscale ip -4)
[[ -n "$address" ]] || { echo "No Tailscale IPv4 address available" >&2; exit 1; }

if [[ $1 == address ]]; then
  printf '%s\n' "$address"
else
  exec "$2" --listen "$address:$3" \
    --listen "127.0.0.1:$3" --listen "[::1]:$3" --allow-non-loopback
fi
