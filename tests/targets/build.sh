#!/bin/sh
set -eu
cd "$(dirname "$0")"
mkdir -p bin
for name in busy_loop sleeping threads allocator recursive mmap_test ipc activity; do
    cc -g -O0 -fno-omit-frame-pointer -fno-optimize-sibling-calls -pthread "$name.c" -o "bin/$name"
done
cc -g -O0 -no-pie -fno-omit-frame-pointer -fno-optimize-sibling-calls recursive.c -o bin/recursive_nopie
cc -g0 -O0 -fno-omit-frame-pointer -fno-optimize-sibling-calls recursive.c -o bin/recursive_nodebug
