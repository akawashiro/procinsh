# Chromium

Browser tests, capture automation, and browser workloads use Chromium. Set
`CHROMIUM` to override its executable path; the default is `/usr/bin/chromium`.
Browser workloads accept `WORKLOAD=chromium` (the default) or `WORKLOAD=firefox`.
The capture script and recording VM also accept `WORKLOAD=kernel-build`.

## Installation

Docker images and recording VMs use `scripts/install_chromium.sh`. On Ubuntu,
it installs native Chromium packages from [the xtradeb apps PPA](https://launchpad.net/~xtradeb/+archive/ubuntu/apps).
The repository has a dedicated signing keyring, checked against its full signing
key fingerprint. APT prefers the PPA for `chromium*` packages only. Both Ubuntu
24.04 and 26.04 have amd64 and arm64 builds. Fedora uses its native Chromium package.

Run `sudo bash scripts/install_chromium.sh` to use the same installation on a host.

## Browser tests

Run `bash scripts/docker_test.sh browser` for the complete browser suite using the
default Ubuntu 24.04 container. Use `--distro ubuntu:26.04` or `--distro fedora:44`
to select another supported distribution.

CI runs the complete browser suite on x86-64 and `browser-replay` on arm64.
`browser-replay` checks static replay, WebGL, recording selection, and navigation
without requiring the unimplemented AArch64 register sampler. The complete live
process browser suite requires x86-64 register capture.
