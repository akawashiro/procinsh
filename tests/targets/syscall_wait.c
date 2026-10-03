#define _GNU_SOURCE
#include <errno.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/epoll.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

static int wait_ms = 1000;

static double now(clockid_t clock) {
    struct timespec ts;
    clock_gettime(clock, &ts);
    return ts.tv_sec + ts.tv_nsec / 1e9;
}

__attribute__((noinline)) static void wait_futex(void) {
    static pthread_mutex_t mutex = PTHREAD_MUTEX_INITIALIZER;
    static pthread_cond_t condition = PTHREAD_COND_INITIALIZER;
    struct timespec deadline;
    clock_gettime(CLOCK_REALTIME, &deadline);
    deadline.tv_sec += wait_ms / 1000;
    deadline.tv_nsec += (wait_ms % 1000) * 1000000L;
    if (deadline.tv_nsec >= 1000000000L) {
        deadline.tv_sec++;
        deadline.tv_nsec -= 1000000000L;
    }
    pthread_mutex_lock(&mutex);
    int result = pthread_cond_timedwait(&condition, &mutex, &deadline);
    pthread_mutex_unlock(&mutex);
    if (result != ETIMEDOUT)
        exit(2);
}
__attribute__((noinline)) static void wait_epoll(int fd) {
    struct epoll_event event;
    if (epoll_wait(fd, &event, 1, wait_ms) != 0)
        exit(2);
}
__attribute__((noinline)) static void wait_sleep(void) {
    struct timespec delay = {wait_ms / 1000, (wait_ms % 1000) * 1000000L};
    nanosleep(&delay, NULL);
}
__attribute__((noinline)) static void syscall_loop(void) { syscall(SYS_getpid); }
__attribute__((noinline)) static void nested(int depth, const char *mode, int fd) {
    if (depth)
        nested(depth - 1, mode, fd);
    else if (!strcmp(mode, "futex"))
        wait_futex();
    else if (!strcmp(mode, "epoll"))
        wait_epoll(fd);
    else if (!strcmp(mode, "busy"))
        syscall_loop();
    else
        wait_sleep();
    __asm__ volatile("" ::: "memory");
}
int main(int argc, char **argv) {
    const char *duration = getenv("PROCINSH_WAIT_MS");
    if (duration)
        wait_ms = atoi(duration);
    if (wait_ms < 1 || wait_ms > 8000)
        return 2;
    const char *mode = argc > 1 ? argv[1] : "sleep";
    int fd = -1;
    if (!strcmp(mode, "epoll")) {
        fd = epoll_create1(EPOLL_CLOEXEC);
        if (fd < 0)
            return 2;
    }
    printf("%d\n", getpid());
    fflush(stdout);
    double begin = now(CLOCK_MONOTONIC), cpu = now(CLOCK_PROCESS_CPUTIME_ID);
    struct timespec initial = {!strcmp(mode, "initial") ? 10 : 0, 300000000};
    nanosleep(&initial, NULL);
    uint64_t iterations = 0;
    while (now(CLOCK_MONOTONIC) - begin < 8) {
        nested(6, mode, fd);
        iterations++;
    }
    printf("iterations=%lu wall=%.6f cpu=%.6f\n", iterations, now(CLOCK_MONOTONIC) - begin,
           now(CLOCK_PROCESS_CPUTIME_ID) - cpu);
    if (fd >= 0)
        close(fd);
    return 0;
}
