#!/bin/bash
set -euo pipefail

state=${PROCINSH_PREVIEW_STATE:-$HOME/procinsh-main-preview}
source_repo=${PROCINSH_PREVIEW_SOURCE:-$HOME/ghq/github.com/akawashiro/procinsh}
service=procinsh-preview.service
mkdir -p "$state"

case "${1:-}" in
  build)
    exec 9>"$state/update.lock"
    flock -n 9 || exit 0
    # A failed build must never leave a candidate for ExecStartPost.
    rm -f "$state/candidate" "$state/candidate.commit"
    git -c credential.helper= -c 'credential.helper=!gh auth git-credential' \
      -C "$source_repo" fetch https://github.com/akawashiro/procinsh.git \
      +refs/heads/main:refs/procinsh-preview/main
    commit=$(git -C "$source_repo" rev-parse refs/procinsh-preview/main)
    if [[ -f "$state/current.commit" && $(cat "$state/current.commit") == "$commit" ]]; then
      echo "Already running main $commit"
      exit 0
    fi
    if [[ ! -d "$state/worktree" ]]; then
      git -C "$source_repo" worktree add --detach "$state/worktree" "$commit"
    else
      git -C "$state/worktree" checkout --detach "$commit"
    fi
    cd "$state/worktree"
    # systemd does not load shell startup files that normally initialize nvm.
    if ! command -v npm >/dev/null 2>&1; then
      export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
      if [[ -s "$NVM_DIR/nvm.sh" ]]; then
        source "$NVM_DIR/nvm.sh" --no-use
        nvm use default
      fi
    fi
    npm ci
    npm run build:web
    cargo build --locked
    install -m 0755 target/debug/procinsh "$state/candidate"
    # Apply privileges before touching the running version; no password prompt.
    sudo -n setcap cap_sys_ptrace,cap_bpf,cap_perfmon=ep "$state/candidate"
    printf '%s\n' "$commit" > "$state/candidate.commit"
    echo "Built main $commit"
    ;;
  activate)
    [[ -f "$state/candidate.commit" ]] || exit 0
    exec 9>"$state/activate.lock"
    flock -n 9 || exit 0
    if [[ -f "$state/procinsh" ]]; then
      # A hard link retains file capabilities for rollback without another sudo.
      ln -f "$state/procinsh" "$state/previous"
      cp "$state/current.commit" "$state/previous.commit"
    fi
    mv "$state/candidate" "$state/procinsh"
    mv "$state/candidate.commit" "$state/current.commit"
    # Probe the actual HTTP server, rather than only systemd's process state.
    if systemctl --user restart "$service"; then
      for _ in {1..20}; do
        if systemctl --user is-active --quiet "$service" && \
          curl --noproxy '*' --fail --silent --max-time 1 http://127.0.0.1:9090/ >/dev/null; then
          echo "Activated main $(cat "$state/current.commit")"
          exit 0
        fi
        sleep 0.5
      done
    fi
    echo "Activation failed for $(cat "$state/current.commit")" >&2
    systemctl --user stop "$service"
    if [[ -f "$state/previous" ]]; then
      mv "$state/previous" "$state/procinsh"
      mv "$state/previous.commit" "$state/current.commit"
      systemctl --user start "$service"
      echo "Restored $(cat "$state/current.commit")" >&2
    else
      rm -f "$state/procinsh" "$state/current.commit"
    fi
    exit 1
    ;;
  *) echo "Usage: $0 {build|activate}" >&2; exit 2 ;;
esac
