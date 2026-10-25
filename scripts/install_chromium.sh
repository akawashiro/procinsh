#!/bin/bash
set -euo pipefail

# Use native Chromium packages in both containers and recording VMs.
. /etc/os-release
if [[ $ID == ubuntu ]]; then
  apt-get update -o APT::Update::Error-Mode=any
  apt-get install -y --no-install-recommends ca-certificates curl gnupg
  key=5301FA4FD93244FBC6F6149982BB6851C64F6880
  temporary=$(mktemp -d)
  trap 'rm -rf "$temporary"' EXIT
  curl -fsSL "https://keyserver.ubuntu.com/pks/lookup?op=get&search=0x$key" \
    -o "$temporary/key.asc"
  fingerprint=$(gpg --show-keys --with-colons "$temporary/key.asc" \
    | awk -F: '$1 == "fpr" {print $10; exit}')
  [[ $fingerprint == "$key" ]] || { echo 'Unexpected Chromium PPA signing key' >&2; exit 1; }
  install -d -m 0755 /etc/apt/keyrings
  gpg --batch --yes --dearmor -o /etc/apt/keyrings/procinsh-chromium.gpg "$temporary/key.asc"
  chmod 0644 /etc/apt/keyrings/procinsh-chromium.gpg
  cat >/etc/apt/sources.list.d/procinsh-chromium.sources <<SOURCE
Types: deb
URIs: https://ppa.launchpadcontent.net/xtradeb/apps/ubuntu
Suites: $VERSION_CODENAME
Components: main
Signed-By: /etc/apt/keyrings/procinsh-chromium.gpg
SOURCE
  # Prefer the PPA only for Chromium, preserving the distribution's other packages.
  cat >/etc/apt/preferences.d/procinsh-chromium <<'PREFERENCES'
Package: *
Pin: release o=LP-PPA-xtradeb-apps
Pin-Priority: 100

Package: chromium*
Pin: release o=LP-PPA-xtradeb-apps
Pin-Priority: 700
PREFERENCES
fi

if command -v apt-get >/dev/null; then
  apt-get update -o APT::Update::Error-Mode=any
  apt-get install -y --no-install-recommends chromium
else
  dnf install -y chromium
fi
