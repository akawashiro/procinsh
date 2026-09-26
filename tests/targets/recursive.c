#include "support.h"
__attribute__((noinline)) void baz(void) { volatile uint64_t counter = 0; for (;;) counter++; }
__attribute__((noinline)) void bar(void) { baz(); asm volatile("" ::: "memory"); }
__attribute__((noinline)) void foo(void) { bar(); asm volatile("" ::: "memory"); }
int main(int argc, char **argv) {
    setup(argc, argv); char message[] = "procinsh recursive fixture"; ready(message); foo(); return 0;
}
