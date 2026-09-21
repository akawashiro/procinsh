#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <unistd.h>
#include <sys/prctl.h>

static void setup(int argc, char **argv) {
    /* Explicit opt-in for a sibling inspector under Yama; test fixtures only. */
    if (argc > 1 && strcmp(argv[1], "--allow-inspector") == 0) {
        if (prctl(PR_SET_PTRACER, PR_SET_PTRACER_ANY, 0, 0, 0)) { perror("prctl"); exit(1); }
    }
    alarm(120);
}
static void ready(void *address) {
    printf("%d %p\n", getpid(), address);
    fflush(stdout);
}
