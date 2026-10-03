#include <time.h>

__attribute__((noinline)) void shared_recurse(int depth) {
    if (depth) {
        shared_recurse(depth - 1);
    } else {
        struct timespec delay = {.tv_sec = 0, .tv_nsec = 100000000};
        nanosleep(&delay, 0);
    }
    asm volatile("" ::: "memory");
}
