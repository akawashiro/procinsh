#include "support.h"
int main(int argc, char **argv) {
    setup(argc, argv); ready(NULL);
    for (;;) {
        char *data = malloc(32*1024*1024); if (!data) return 1;
        memset(data, 'A', 32*1024*1024); sleep(1); free(data); sleep(1);
    }
}
