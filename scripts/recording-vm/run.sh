#!/bin/bash

# Dedicated, disposable recording machine. No SSH forwarding or host filesystem sharing.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
state=${VM_DIR:-/tmp/procinsh-recording-vm}
port=${HOST_PORT:-10090}
cpus=${VM_CPUS:-4}
memory=${VM_MEMORY_MIB:-8192}
image_base=${IMAGE_BASE:-https://cloud-images.ubuntu.com/releases/26.04/release-20260927}
image_name=ubuntu-26.04-server-cloudimg-amd64.img
case "$state" in *[[:space:],]*) echo 'VM_DIR must not contain whitespace or commas' >&2; exit 2 ;; esac
case ${1:-} in
  prepare|run|stop) ;;
  *) echo "Usage: $0 {prepare|run|stop}" >&2; exit 2 ;;
esac
mkdir -p "$state"
state=$(cd "$state" && pwd)
if [[ $1 == stop ]]; then
  python3 - "$state/monitor.sock" <<'PY'
import socket, sys
with socket.socket(socket.AF_UNIX) as monitor:
    monitor.connect(sys.argv[1])
    monitor.sendall(b'system_powerdown\n')
PY
  exit
fi
if [[ $1 == run ]]; then
  [[ -f $state/http-port ]] || { echo 'Run prepare first' >&2; exit 1; }
  prepared_port=$(cat "$state/http-port")
  [[ ${HOST_PORT:-$prepared_port} == "$prepared_port" ]] || {
    echo "HOST_PORT must match the prepared guest port ($prepared_port); prepare a new VM_DIR to change it" >&2; exit 2;
  }
  port=$prepared_port
fi
[[ $port =~ ^[0-9]+$ ]] && (( 10#$port >= 1 && 10#$port <= 65535 )) || {
  echo 'HOST_PORT must be an integer between 1 and 65535' >&2; exit 2;
}
port=$((10#$port))
required_tools=(qemu-system-x86_64 python3 flock)
if [[ $1 == prepare ]]; then required_tools+=(qemu-img cloud-localds curl); fi
for tool in "${required_tools[@]}"; do
  command -v "$tool" >/dev/null || { echo "Missing tool: $tool" >&2; exit 1; }
done
# Keep another invocation from modifying a running guest or preparing its disk.
exec 9>"$state/lock"
flock -n 9 || { echo "VM directory is busy: $state" >&2; exit 1; }
if [[ $1 == prepare ]]; then
  [[ ! -e $state/guest.qcow2 ]] || { echo "Existing guest: choose a fresh VM_DIR" >&2; exit 1; }
  curl -fL "$image_base/SHA256SUMS" -o "$state/SHA256SUMS"
  curl -fL "$image_base/$image_name" -o "$state/$image_name.partial"
  python3 - "$state" "$image_name" <<'PY'
import hashlib, pathlib, sys
root, name = pathlib.Path(sys.argv[1]), sys.argv[2]
expected = [line.split()[0] for line in (root/'SHA256SUMS').read_text().splitlines()
            if line.split()[-1].lstrip('*') == name]
with (root/(name+'.partial')).open('rb') as image:
    actual = hashlib.file_digest(image, 'sha256').hexdigest()
if expected != [actual]:
    raise SystemExit('Cloud image SHA256 mismatch')
(root/(name+'.partial')).rename(root/name)
PY
  qemu-img create -f qcow2 -F qcow2 -b "$state/$image_name" "$state/guest.qcow2" "${VM_DISK_GIB:-40}G"
  python3 "$root/scripts/recording-vm/seed.py" "$state" "${PROCINSH_REF:-$(git -C "$root" rev-parse HEAD)}" "$port"
  cloud-localds "$state/seed.img" "$state/user-data" "$state/meta-data"
  echo "Prepared $state. Run: VM_DIR=$state $0 run"
  exit
fi
[[ -r /dev/kvm && -w /dev/kvm ]] || { echo 'KVM is required: /dev/kvm must be readable and writable' >&2; exit 1; }
[[ -f $state/seed.img && -f $state/guest.qcow2 ]] || { echo 'Run prepare first' >&2; exit 1; }
python3 - "$port" "$cpus" "$memory" <<'PY'
import socket, sys
port, cpus, memory = map(int, sys.argv[1:])
if not (1 <= port <= 65535 and 1 <= cpus <= 256 and memory >= 2048):
    raise SystemExit('Invalid HOST_PORT, VM_CPUS or VM_MEMORY_MIB')
with socket.socket() as probe:
    try:
        probe.bind(('127.0.0.1', port))
    except OSError as error:
        raise SystemExit(f'HOST_PORT {port} is unavailable: {error}')
PY
printf 'Open http://127.0.0.1:%s/space after cloud-init finishes. Console log: %s/console.log\n' "$port" "$state"
exec qemu-system-x86_64 -enable-kvm -cpu host -smp "$cpus" -m "$memory" \
  -drive "file=$state/guest.qcow2,format=qcow2,if=virtio" \
  -drive "file=$state/seed.img,format=raw,if=virtio,readonly=on" \
  -nic "user,model=virtio-net-pci,hostfwd=tcp:127.0.0.1:$port-:$port" \
  -monitor "unix:$state/monitor.sock,server=on,wait=off" \
  -chardev "socket,id=console,path=$state/console.sock,server=on,wait=off,logfile=$state/console.log,logappend=on" \
  -serial chardev:console -display none
