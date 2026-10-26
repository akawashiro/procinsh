#!/bin/bash
set -euo pipefail
exec > >(tee /var/log/recording-setup.log) 2>&1
port=$2
workload=$3
test -r /sys/kernel/btf/vmlinux
git clone https://github.com/akawashiro/procinsh.git /opt/procinsh
cd /opt/procinsh
git checkout --detach "$1"
curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o /tmp/rustup.sh
RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo sh /tmp/rustup.sh -y --default-toolchain none
export RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
npm --prefix web ci
npm --prefix web run build
cargo build --locked
if [[ $workload == kernel-build ]]; then
  sudo -u demo /opt/kernel-build.sh prepare
elif [[ $workload == firefox ]]; then
  apt-get install -y --no-install-recommends libgtk-3-0t64 libdbus-glib-1-2 libasound2t64 libx11-xcb1 libxt6t64 xz-utils
  firefox_version=158.0
  firefox_base="https://archive.mozilla.org/pub/firefox/releases/$firefox_version"
  archive="linux-x86_64/en-US/firefox-$firefox_version.tar.xz"
  curl -fL "$firefox_base/$archive" -o /tmp/firefox.tar.xz
  curl -fL "$firefox_base/SHA512SUMS" -o /tmp/firefox-SHA512SUMS
  python3 - "$archive" <<'PYTHON'
import hashlib, pathlib, sys
expected = [line.split()[0] for line in pathlib.Path('/tmp/firefox-SHA512SUMS').read_text().splitlines() if line.split()[-1].lstrip('*') == sys.argv[1]]
with open('/tmp/firefox.tar.xz', 'rb') as archive:
    actual = hashlib.file_digest(archive, 'sha512').hexdigest()
if expected != [actual]:
    raise SystemExit('Firefox SHA512 mismatch')
PYTHON
  tar -xJf /tmp/firefox.tar.xz -C /opt
else
  bash scripts/install_chromium.sh
fi
cat >/etc/systemd/system/space-demo.service <<UNIT
[Unit]
Description=Dedicated SPACE recording collector
After=network.target
[Service]
WorkingDirectory=/opt/procinsh
ExecStart=/opt/procinsh/scripts/dev_run.sh --listen 0.0.0.0:$port --allow-non-loopback
Restart=on-failure
[Install]
WantedBy=multi-user.target
UNIT
if [[ $workload == kernel-build ]]; then
  cat >/etc/systemd/system/kernel-build-demo.service <<UNIT
[Unit]
Description=Linux kernel compilation for SPACE recordings
After=space-demo.service
[Service]
User=demo
ExecStart=/opt/kernel-build.sh build
StandardOutput=append:/home/demo/kernel-build.log
StandardError=append:/home/demo/kernel-build.log
[Install]
WantedBy=multi-user.target
UNIT
  workload_service=kernel-build-demo
else
  cat >/etc/systemd/system/browser-demo.service <<UNIT
[Unit]
Description=External-site browser workload for SPACE recordings
After=network.target
[Service]
User=demo
WorkingDirectory=/opt/procinsh
Environment=WORKLOAD=$workload TAB_COUNT=8 INTERVAL_SECONDS=10
ExecStart=/usr/bin/node scripts/browser_workload.mjs
Restart=on-failure
[Install]
WantedBy=multi-user.target
UNIT
  workload_service=browser-demo
fi
# The console is local to QEMU; there is no SSH forwarding or password login.
mkdir -p /etc/systemd/system/serial-getty@ttyS0.service.d
printf '[Service]\nExecStart=\nExecStart=-/sbin/agetty --autologin demo --noclear %%I $TERM\n' >/etc/systemd/system/serial-getty@ttyS0.service.d/autologin.conf
systemctl daemon-reload
systemctl restart serial-getty@ttyS0
systemctl enable --now space-demo "$workload_service"
echo "SPACE recording ready on guest port $port"
