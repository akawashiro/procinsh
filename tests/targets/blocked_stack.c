#include "support.h"
#include <time.h>

/* Known GPR values at a real blocking syscall, independent of the perf parser. */
__attribute__((noinline)) static void blocked_leaf(void) {
    struct timespec delay = {.tv_sec = 0, .tv_nsec = 100000000};
    asm volatile("mov $0x12345678, %%r12\n\t"
                 "mov $0x23456789, %%r13\n\t"
                 "mov $0x3456789a, %%r14\n\t"
                 "mov $0x456789ab, %%r15\n\t"
                 "mov $35, %%rax\n\t"
                 "xor %%rsi, %%rsi\n\t"
                 "syscall"
                 :
                 : "D"(&delay)
                 : "rax", "rsi", "rcx", "r11", "r12", "r13", "r14", "r15", "memory");
}
__attribute__((noinline)) static void blocked_recurse(int depth) {
    if (depth)
        blocked_recurse(depth - 1);
    else
        blocked_leaf();
    asm volatile("" ::: "memory");
}
int main(int argc, char **argv) {
    setup(argc, argv);
    ready(NULL);
    for (;;) {
        blocked_recurse(12);
        blocked_recurse(96);
    }
}
