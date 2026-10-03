#!/bin/sh
set -eu
cd "$(dirname "$0")"
mkdir -p bin
for name in busy_loop blocked_stack sleeping threads allocator recursive mmap_test ipc activity; do
    cc -g -O0 -fno-omit-frame-pointer -fno-optimize-sibling-calls -pthread "$name.c" -o "bin/$name"
done
cc -g -O0 -no-pie -fno-omit-frame-pointer -fno-optimize-sibling-calls recursive.c -o bin/recursive_nopie
cc -g0 -O0 -fno-omit-frame-pointer -fno-optimize-sibling-calls recursive.c -o bin/recursive_nodebug

cc -g -O2 -fomit-frame-pointer -fno-optimize-sibling-calls recursive.c -o bin/recursive_no_fp
cc -g -O2 -no-pie -fomit-frame-pointer -fno-optimize-sibling-calls recursive.c -o bin/recursive_no_fp_nopie
cc -g -O2 -fomit-frame-pointer -fno-optimize-sibling-calls -fPIC -shared unwind_shared.c -o bin/libunwind_fixture.so
cc -g -O2 -fomit-frame-pointer -fno-optimize-sibling-calls shared_no_fp.c -Lbin -lunwind_fixture -Wl,-rpath,'$ORIGIN' -o bin/shared_no_fp
cc -g -O2 -fomit-frame-pointer -fno-optimize-sibling-calls -fno-asynchronous-unwind-tables -fno-unwind-tables recursive.c -o bin/recursive_debug_frame
