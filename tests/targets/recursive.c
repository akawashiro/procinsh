#include "support.h"
__attribute__((noinline)) void baz(void) { for (;;) usleep(10000); }
__attribute__((noinline)) void bar(void) { baz(); asm volatile("" ::: "memory"); }
__attribute__((noinline)) void foo(void) { bar(); asm volatile("" ::: "memory"); }
int main(int argc, char **argv) {
    setup(argc, argv); char message[] = "procinsh recursive fixture"; ready(message); foo(); return 0;
}
