#include "support.h"
#include <pthread.h>
#include <signal.h>
#include <stdatomic.h>
#include <sys/mman.h>
static volatile sig_atomic_t execute;
static _Atomic unsigned long progress;
static int dynamic_code;
static int sleeping_workers;
static void on_signal(int sig) { (void)sig; execute = 1; }
static void *work(void *unused) {
    (void)unused;
    for (;;) {
        if (sleeping_workers) {
            usleep(10000);
        } else if (dynamic_code) {
            unsigned char *code = mmap(NULL, 4096, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
            if (code == MAP_FAILED) abort();
            /* mov ecx, 100000; dec ecx; jnz -4; ret */
            const unsigned char loop[] = {0xb9,0xa0,0x86,0x01,0x00,0xff,0xc9,0x75,0xfc,0xc3};
            memcpy(code,loop,sizeof(loop));
            if (mprotect(code,4096,PROT_READ|PROT_EXEC)) abort();
            ((void (*)(void))code)(); munmap(code,4096);
        } else {
            for (volatile unsigned i=0;i<100000;i++) {}
        }
        atomic_fetch_add_explicit(&progress,1,memory_order_relaxed);
    }
    return NULL;
}
int main(int argc, char **argv) {
    setup(argc,argv); signal(SIGUSR1,on_signal);
    int count = argc>2 ? atoi(argv[2]) : 1;
    dynamic_code = argc>3 && !strcmp(argv[3],"jit");
    sleeping_workers = argc>3 && !strcmp(argv[3],"sleep");
    if (count<1 || count>128) return 2;
    for(int i=0;i<count;i++) {pthread_t t; if(pthread_create(&t,NULL,work,NULL))return 1;}
    ready(&progress);
    for (;;) {
        if (execute) execl("/proc/self/exe",argv[0],"--allow-inspector","1",NULL);
        usleep(10000);
    }
}
