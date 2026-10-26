#!/bin/bash
set -euo pipefail

# All writes, including fixture builds, stay in the container.
tar -C /source --exclude='./target' --exclude='./web/node_modules' \
  --exclude='./web/dist' --exclude='./tests/targets/bin' \
  --exclude='./.venv-format' --exclude='./.aws' --exclude='./.codex' \
  --exclude='./.agents' -cf - . | tar -C /work -xf -
git config --global --add safe.directory /work
test -r /sys/kernel/btf/vmlinux || {
  echo 'The host kernel must expose /sys/kernel/btf/vmlinux (CONFIG_DEBUG_INFO_BTF=y).' >&2
  exit 1
}
bpftool version
npm --prefix web ci
npm --prefix web run build

suite=$1
shift
case "$suite" in
  ordinary)
    npm --prefix web run build:replay
    cargo build --locked --all-targets
    cargo test --locked --no-run
    # Use nobody (UID/GID 65534) so root-launched tests exercise a user-owned helper.
    chown 65534:65534 target/debug/procinsh-privileged-helper
    setcap cap_sys_ptrace,cap_bpf,cap_perfmon,cap_dac_read_search,cap_sys_admin=ep target/debug/procinsh-privileged-helper
    cargo test --locked
    npm --prefix web test
    python3 tests/listen-policy.py
    ;;
  checks)
    bash -n scripts/docker_test.sh scripts/docker/entrypoint.sh scripts/install_chromium.sh scripts/recording-vm/*.sh \
      scripts/dev_run.sh users/akawashiro/scripts/{main_preview,release_preview}.sh
    node --check scripts/chromium_load.mjs
    node --check scripts/browser_workload.mjs
    node --check web/scripts/browser-workload.mjs
    npm --prefix web run format:check
    cargo fmt --all --check
    python3 scripts/format_c.py --check
    cargo clippy --locked --all-targets -- -D warnings
    RUSTDOCFLAGS='-D rustdoc::broken_intra_doc_links' \
      cargo doc --locked --workspace --no-deps --document-private-items
    npm --prefix web run docs
    npm --prefix web run docs:architecture
    python3 scripts/check_web_package.py
    ;;
  kernel)
    cargo build --locked
    # Use nobody (UID/GID 65534) so root-launched tests exercise a user-owned helper.
    chown 65534:65534 target/debug/procinsh-privileged-helper
    setcap cap_sys_ptrace,cap_bpf,cap_perfmon,cap_dac_read_search,cap_sys_admin=ep target/debug/procinsh-privileged-helper
    sh tests/targets/build.sh
    log=$(mktemp)
    target/debug/procinsh --listen 127.0.0.1:0 >"$log" 2>&1 &
    server=$!
    cleanup() {
      kill "$server" 2>/dev/null || true
      wait "$server" 2>/dev/null || true
      cat "$log"
      rm -f "$log"
    }
    trap cleanup EXIT
    python3 - "$log" "$server" <<'PYTHON'
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import threading
import time
sys.path.insert(0, 'tests')
from sse import Stream

for _ in range(200):
    os.kill(int(sys.argv[2]), 0)
    match = re.search(r'listening on (http://127\.0\.0\.1:\d+)', Path(sys.argv[1]).read_text())
    if match:
        base = match[1]
        break
    time.sleep(0.05)
else:
    raise SystemExit('Live test server did not become ready')

# Keep a viewer connected before spawning fixtures: attaching an fexit program
# cannot observe a pipe read that entered the kernel before the attachment.
stream = Stream(base + '/api/system/events')
reader = None
try:
    deadline = time.monotonic() + 20
    for line in stream:
        if time.monotonic() >= deadline:
            raise RuntimeError("Live sensors did not become ready within 20 seconds")
        if not line.startswith(b'data: '):
            continue
        status = json.loads(line[6:]).get('status', {})
        states = [status.get(name, {}).get('state') for name in ('cpu', 'ipc', 'files', 'signals')]
        if any(state in ('unavailable', 'error') for state in states):
            raise RuntimeError(f'Live sensors are required: {status}')
        if all(state == 'observing' for state in states):
            break
    else:
        raise RuntimeError('Live sensor stream ended before readiness')

    def drain():
        try:
            for _ in stream:
                pass
        except (OSError, ValueError):
            pass

    reader = threading.Thread(target=drain, daemon=True)
    reader.start()
    for test in ('space-live.py', 'space-files-live.py', 'space-signals-live.py'):
        subprocess.run([sys.executable, 'tests/' + test, base], check=True)
finally:
    stream.close(reader)
PYTHON
    ;;
  browser)
    cargo build --locked
    # Use nobody (UID/GID 65534) so root-launched tests exercise a user-owned helper.
    chown 65534:65534 target/debug/procinsh-privileged-helper
    setcap cap_sys_ptrace,cap_bpf,cap_perfmon,cap_dac_read_search,cap_sys_admin=ep target/debug/procinsh-privileged-helper
    sh tests/targets/build.sh
    CHROMIUM=$(command -v chromium || command -v chromium-browser)
    export CHROMIUM
    npm --prefix web run test:browser
    ;;
  browser-replay)
    CHROMIUM=$(command -v chromium || command -v chromium-browser)
    export CHROMIUM
    npm --prefix web run test:browser:replay
    ;;
  rust)
    cargo build --locked
    cargo test --locked --no-run
    # Use nobody (UID/GID 65534) so root-launched tests exercise a user-owned helper.
    chown 65534:65534 target/debug/procinsh-privileged-helper
    setcap cap_sys_ptrace,cap_bpf,cap_perfmon,cap_dac_read_search,cap_sys_admin=ep target/debug/procinsh-privileged-helper
    cargo test --locked -- "$@" ;;
  web) npm --prefix web test -- "$@" ;;
  *) echo "Unknown container test suite: $suite" >&2; exit 2 ;;
esac
