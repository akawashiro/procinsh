#!/bin/bash
set -euo pipefail

state=${PROCINSH_RELEASE_PREVIEW_STATE:-$HOME/procinsh-release-preview}
service=procinsh-release-preview.service

if [[ ${1:-} != update ]]; then
  echo "Usage: $0 update" >&2
  exit 2
fi

mkdir -p "$state"
exec 9>"$state/update.lock"
flock -n 9 || exit 0
# Keep one lock through installation, activation, and rollback.
rm -f "$state/candidate" "$state/candidate.version"
cd "$state"
# Keep Cargo's install metadata separate from the active binary. After a failed
# activation Cargo can reuse the installed candidate on the next attempt.
CARGO_TARGET_DIR="$state/target" cargo install procinsh --locked \
  --registry crates-io --root "$state/install"
version=$(cargo install --list --root "$state/install" | sed -n 's/^procinsh v\(.*\):$/\1/p')
[[ -n "$version" ]] || { echo "Could not determine installed procinsh version" >&2; exit 1; }
if [[ -f "$state/procinsh" ]] && cmp -s "$state/install/bin/procinsh" "$state/procinsh"; then
  echo "Already running release $version"
  exit 0
fi

install -m 0755 "$state/install/bin/procinsh" "$state/candidate"
sudo -n setcap cap_sys_ptrace,cap_bpf,cap_perfmon,cap_dac_read_search=ep "$state/candidate"
printf '%s\n' "$version" > "$state/candidate.version"
if [[ -f "$state/procinsh" ]]; then
  # A hard link retains file capabilities for rollback without another sudo.
  ln -f "$state/procinsh" "$state/previous"
  cp "$state/current.version" "$state/previous.version"
else
  rm -f "$state/previous" "$state/previous.version"
fi
mv "$state/candidate" "$state/procinsh"
mv "$state/candidate.version" "$state/current.version"
if systemctl --user restart "$service"; then
  for _ in {1..20}; do
    if systemctl --user is-active --quiet "$service" && \
      curl --noproxy '*' --fail --silent --max-time 1 http://127.0.0.1:9091/ >/dev/null; then
      echo "Activated release $version"
      exit 0
    fi
    sleep 0.5
  done
fi
echo "Activation failed for release $version" >&2
systemctl --user stop "$service"
if [[ -f "$state/previous" ]]; then
  mv "$state/previous" "$state/procinsh"
  mv "$state/previous.version" "$state/current.version"
  systemctl --user start "$service"
  echo "Restored release $(cat "$state/current.version")" >&2
else
  rm -f "$state/procinsh" "$state/current.version"
fi
exit 1
