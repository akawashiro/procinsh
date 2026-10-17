#!/bin/bash
set -euo pipefail
cp -a /experiment/source/. /work/
export PROCINSH_GIT_SHA=7bb1b8a13cafbb0d6ca31b549dd507fdd8ed9036
npm --prefix web ci
npm --prefix web run build
cargo build --release --locked
cp target/release/procinsh /experiment/bin/experiment-release
cp target/release/procinsh /experiment/runtime/experiment-release/target/debug/procinsh
chmod a+rx /experiment/bin/experiment-release /experiment/runtime/experiment-release/target/debug/procinsh
