#!/bin/bash
set -euo pipefail

case "${1:-}" in
  address) [[ $# == 1 ]] ;;
  run) [[ $# == 3 ]] ;;
  *) echo "Usage: $0 {address|run BINARY PORT}" >&2; exit 2 ;;
esac

if [[ $1 == address ]]; then
  printf '%s\n' '127.0.0.1'
else
  exec "$2" --listen "127.0.0.1:$3" --listen "[::1]:$3"
fi
