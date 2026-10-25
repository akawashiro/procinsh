#!/bin/bash
set -euo pipefail

usage() {
  echo "Usage: bash scripts/docker_test.sh [--distro ubuntu:24.04|ubuntu:26.04|fedora:44] [--network NETWORK] [all|ordinary|kernel|checks|browser|browser-replay|rust|web] [test filters...]"
}
distro=ubuntu:24.04
network=bridge
while [[ ${1:-} == --* ]]; do
  case "$1" in
    --distro|--network)
      [[ $# -ge 2 ]] || { usage >&2; exit 2; }
      if [[ $1 == --distro ]]; then distro=$2; else network=$2; fi
      shift 2
      ;;
    --help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done
case "$distro" in
  ubuntu:24.04|ubuntu:26.04|fedora:44) ;;
  *) usage >&2; exit 2 ;;
esac
suite=${1:-all}
if [[ $# -gt 0 ]]; then shift; fi
case "$suite" in
  all|ordinary|kernel|checks|browser|browser-replay) [[ $# == 0 ]] || { usage >&2; exit 2; } ;;
  rust|web) ;;
  -h|--help) usage; exit 0 ;;
  *) usage >&2; exit 2 ;;
esac
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
key=${distro//[:.]/-}
image="procinsh-tests:$key"
build_options=()
# BuildKit calls its default network "default"; Docker run calls it "bridge".
if [[ $network != bridge ]]; then build_options+=(--network "$network"); fi
docker build "${build_options[@]}" --file "$root/scripts/docker/Dockerfile" \
  --build-arg "BASE_IMAGE=$distro" \
  --tag "$image" "$root"

run_suite() {
  local selected=$1
  shift
  local options=()
  case "$selected" in
    ordinary|kernel|rust|browser)
      options+=(--cap-add=BPF --cap-add=PERFMON --cap-add=SYS_PTRACE
        --cap-add=DAC_READ_SEARCH --cap-add=SYS_ADMIN)
      ;;
  esac
  case "$selected" in
    kernel|browser)
      # BPF events carry host PIDs; procfs must use the same IDs as the sensors.
      options+=(--pid=host)
      ;;
  esac
  # The checkout is read-only. Generated files and root-owned caches live only
  # in the container and Docker volumes, never in the developer's checkout.
  # A git worktree's .git file names the main repository's git directory; mount it
  # at the same path so the copied checkout can still resolve its commit.
  if [[ -f $root/.git ]]; then
    local common
    common=$(git -C "$root" rev-parse --path-format=absolute --git-common-dir)
    options+=(--mount "type=bind,src=$common,dst=$common,readonly")
  fi
  docker run --rm --init --network "$network" "${options[@]}" \
    --mount "type=bind,src=$root,dst=/source,readonly" \
    --mount type=bind,src=/sys/kernel/btf,dst=/sys/kernel/btf,readonly \
    --mount "type=volume,src=procinsh-test-target-$key,dst=/work/target" \
    --mount type=volume,src=procinsh-test-registry,dst=/opt/cargo/registry \
    "$image" "$selected" "$@"
}
if [[ $suite == all ]]; then
  run_suite checks
  run_suite ordinary
  run_suite kernel
else
  run_suite "$suite" "$@"
fi
