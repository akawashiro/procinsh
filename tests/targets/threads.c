#include "support.h"
#include <pthread.h>
#include <signal.h>
static volatile sig_atomic_t sleep_worker;
static void sleep_one_worker(int signal_number) {
    (void)signal_number;
    sleep_worker = 1;
}
static void *worker(void *index) {
    int sleeper = (intptr_t)index == 0;
    pthread_setname_np(pthread_self(), sleeper ? "procinsh-sleep" : "procinsh-worker");
    for (;;) {
        if (sleeper && sleep_worker)
            sleep(1);
        else
            asm volatile("" ::: "memory");
    }
    return NULL;
}
static void *short_lived(void *unused) {
    (void)unused;
    usleep(1000);
    return NULL;
}
static void *churn(void *unused) {
    (void)unused;
    for (;;) {
        pthread_t t;
        if (!pthread_create(&t, NULL, short_lived, NULL))
            pthread_join(t, NULL);
    }
    return NULL;
}
int main(int argc, char **argv) {
    setup(argc, argv);
    signal(SIGUSR1, sleep_one_worker);
    pthread_t ts[5];
    for (int i = 0; i < 4; i++)
        if (pthread_create(&ts[i], NULL, worker, (void *)(intptr_t)i))
            return 1;
    if (pthread_create(&ts[4], NULL, churn, NULL))
        return 1;
    ready(NULL);
    for (;;)
        sleep(1);
}
