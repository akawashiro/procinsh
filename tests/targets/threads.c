#include "support.h"
#include <pthread.h>
static void *worker(void *unused) {
    (void)unused; pthread_setname_np(pthread_self(), "alpha-worker");
    for (;;) usleep(10000);
    return NULL;
}
static void *short_lived(void *unused) { (void)unused; usleep(1000); return NULL; }
static void *churn(void *unused) {
    (void)unused;
    for (;;) { pthread_t t; if (!pthread_create(&t, NULL, short_lived, NULL)) pthread_join(t, NULL); }
    return NULL;
}
int main(int argc, char **argv) {
    setup(argc, argv); pthread_t ts[5];
    for (int i=0; i<4; i++) if (pthread_create(&ts[i], NULL, worker, NULL)) return 1;
    if (pthread_create(&ts[4], NULL, churn, NULL)) return 1;
    ready(NULL); for (;;) sleep(1);
}
