#!/bin/bash
# Install an immutable helper revision under a protected, trusted directory.
set -euo pipefail
[[ $# == 1 ]] || { echo "Usage: $0 HELPER_BINARY" >&2; exit 2; }
[[ $EUID == 0 ]] || { echo 'Run as root.' >&2; exit 1; }
source_binary=$(realpath "$1")
main_binary=$(dirname "$source_binary")/procinsh
revision=$(sha256sum "$main_binary" "$source_binary" | cut -d ' ' -f 1 | sha256sum | cut -d ' ' -f 1)
base=/usr/local/libexec/procinsh
# Do not follow pre-existing symlinks or trust writable ancestors.
for directory in /usr /usr/local /usr/local/libexec "$base"; do
  [[ ! -L $directory ]] || { echo "Symlink not permitted: $directory" >&2; exit 1; }
  if [[ ! -e $directory ]]; then install -d -o root -g root -m 0755 "$directory"; fi
  [[ $(stat -c %u "$directory") == 0 ]] && (( (8#$(stat -c %a "$directory") & 8#022) == 0 )) || exit 1
done
pair="$base/$revision"
helper="$pair/procinsh-privileged-helper"
if [[ ! -d $pair ]]; then
  temporary=$(mktemp -d "$base/.pair.XXXXXXXX")
  trap 'rm -rf "$temporary"' EXIT
  install -o root -g root -m 0755 "$source_binary" "$temporary/procinsh-privileged-helper"
  install -o root -g root -m 0755 "$main_binary" "$temporary/procinsh"
  [[ $(sha256sum "$temporary/procinsh" "$temporary/procinsh-privileged-helper" | cut -d ' ' -f 1 | sha256sum | cut -d ' ' -f 1) == "$revision" ]] || exit 1
  chmod 0755 "$temporary"
  setcap cap_sys_ptrace,cap_bpf,cap_perfmon,cap_dac_read_search,cap_sys_admin=ep "$temporary/procinsh-privileged-helper"
  mv -T "$temporary" "$pair"
fi
printf '%s\n' "$helper"
