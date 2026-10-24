#include "support.h"
#include <time.h>

/* Known GPR values at a real blocking syscall, independent of the perf parser. */
__attribute__((noinline)) static void blocked_leaf(void) {
    struct timespec delay = {.tv_sec = 0, .tv_nsec = 100000000};
#if defined(__x86_64__)
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
#elif defined(__aarch64__)
    /* Callee-saved X19-X22 hold the same values across nanosleep (syscall 101). */
    register const struct timespec *request asm("x0") = &delay;
    asm volatile("movz x19, #0x5678\n\t"
                 "movk x19, #0x1234, lsl #16\n\t"
                 "movz x20, #0x6789\n\t"
                 "movk x20, #0x2345, lsl #16\n\t"
                 "movz x21, #0x789a\n\t"
                 "movk x21, #0x3456, lsl #16\n\t"
                 "movz x22, #0x89ab\n\t"
                 "movk x22, #0x4567, lsl #16\n\t"
                 "mov x1, xzr\n\t"
                 "mov x8, #101\n\t"
                 "svc #0"
                 : "+r"(request)
                 :
                 : "x1", "x8", "x19", "x20", "x21", "x22", "memory");
#else
#error "blocked_stack supports x86-64 and AArch64"
#endif
}
__attribute__((noinline)) static void blocked_recurse(int depth) {
    volatile char padding[256];
    padding[0] = (char)depth;
    if (depth)
        blocked_recurse(depth - 1);
    else
        blocked_leaf();
    asm volatile("" : : "m"(padding) : "memory");
}
int main(int argc, char **argv) {
    setup(argc, argv);
    ready(NULL);
    for (;;) {
        blocked_recurse(12);
        blocked_recurse(96);
    }
}
