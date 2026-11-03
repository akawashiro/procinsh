#include "support.h"

#include <pthread.h>

static pthread_mutex_t mutex = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t condition = PTHREAD_COND_INITIALIZER;
static int worker_tid;
static int stopping;

static void check(int result) {
    if (result) {
        fprintf(stderr, "pthread: %s\n", strerror(result));
        exit(1);
    }
}

static void *worker(void *unused) {
    (void)unused;
    check(pthread_mutex_lock(&mutex));
    worker_tid = gettid();
    check(pthread_cond_signal(&condition));
    while (!stopping)
        check(pthread_cond_wait(&condition, &mutex));
    check(pthread_mutex_unlock(&mutex));
    return NULL;
}

int main(int argc, char **argv) {
    setup(argc, argv);
    puts("ready");
    fflush(stdout);
    pthread_t thread;
    int running = 0;
    char command[32];
    while (fgets(command, sizeof(command), stdin)) {
        if (!strcmp(command, "start\n") && !running) {
            check(pthread_mutex_lock(&mutex));
            worker_tid = 0;
            stopping = 0;
            check(pthread_create(&thread, NULL, worker, NULL));
            while (!worker_tid)
                check(pthread_cond_wait(&condition, &mutex));
            check(pthread_mutex_unlock(&mutex));
            running = 1;
        } else if (!strcmp(command, "stop\n") && running) {
            check(pthread_mutex_lock(&mutex));
            stopping = 1;
            check(pthread_cond_signal(&condition));
            check(pthread_mutex_unlock(&mutex));
            check(pthread_join(thread, NULL));
            running = 0;
        } else {
            fprintf(stderr, "Unexpected lifecycle command: %s", command);
            return 1;
        }
        printf("%d\n", worker_tid);
        fflush(stdout);
    }
    return 0;
}
