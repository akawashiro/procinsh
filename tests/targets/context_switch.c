#define _GNU_SOURCE
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static double now(clockid_t clock) {
    struct timespec ts;
    clock_gettime(clock, &ts);
    return ts.tv_sec + ts.tv_nsec / 1e9;
}

__attribute__((noinline)) static void wait_once(const char *mode, int fd) {
    if (!strcmp(mode, "busy")) {
        volatile uint64_t value = 0;
        for (int i = 0; i < 100000; i++)
            value += i;
    } else if (!strcmp(mode, "fsync")) {
        if (write(fd, "x", 1) != 1 || fsync(fd))
            exit(2);
    } else if (!strcmp(mode, "socket")) {
        char byte;
        if (recv(fd, &byte, 1, 0) != 1)
            exit(2);
    } else {
        struct timespec delay = {!strcmp(mode, "oneshot") ? 6 : 0,
                                 !strcmp(mode, "oneshot") ? 0 : 1000000};
        nanosleep(&delay, NULL);
    }
}

__attribute__((noinline)) static void nested(int depth, const char *mode, int fd) {
    if (depth)
        nested(depth - 1, mode, fd);
    else
        wait_once(mode, fd);
    __asm__ volatile("" ::: "memory");
}

int main(int argc, char **argv) {
    const char *mode = argc > 1 ? argv[1] : "sleep";
    int fd = -1;
    pid_t writer = -1;
    if (!strcmp(mode, "fsync")) {
        const char *directory = getenv("PROCINSH_POC_IO_DIR");
        if (directory) {
            char *path;
            if (asprintf(&path, "%s/procinsh-fsync-XXXXXX", directory) < 0)
                return 2;
            fd = mkstemp(path);
            if (fd < 0) {
                free(path);
                return 2;
            }
            unlink(path);
            free(path);
        } else {
            FILE *file = tmpfile();
            if (!file)
                return 2;
            fd = fileno(file);
        }
    } else if (!strcmp(mode, "socket")) {
        int pair[2];
        if (socketpair(AF_UNIX, SOCK_STREAM, 0, pair))
            return 2;
        writer = fork();
        if (writer < 0)
            return 2;
        if (!writer) {
            close(pair[0]);
            for (;;) {
                struct timespec delay = {0, 1000000};
                nanosleep(&delay, NULL);
                if (send(pair[1], "x", 1, MSG_NOSIGNAL) != 1)
                    _exit(0);
            }
        }
        close(pair[1]);
        fd = pair[0];
    }
    printf("%d\n", getpid());
    fflush(stdout);
    double begin = now(CLOCK_MONOTONIC), cpu = now(CLOCK_PROCESS_CPUTIME_ID);
    if (!strcmp(mode, "initial"))
        sleep(10);
    if (!strcmp(mode, "oneshot")) {
        struct timespec delay = {0, 500000000};
        nanosleep(&delay, NULL);
    }
    uint64_t iterations = 0;
    while (now(CLOCK_MONOTONIC) - begin < 8) {
        nested(6, mode, fd);
        iterations++;
        if (!strcmp(mode, "oneshot"))
            break;
    }
    printf("iterations=%lu wall=%.6f cpu=%.6f\n", iterations, now(CLOCK_MONOTONIC) - begin,
           now(CLOCK_PROCESS_CPUTIME_ID) - cpu);
    if (fd >= 0)
        close(fd);
    if (writer > 0)
        waitpid(writer, NULL, 0);
    return 0;
}
