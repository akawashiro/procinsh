#include "support.h"
#include <sys/mman.h>
int main(int argc, char **argv) {
    setup(argc, argv); size_t page = (size_t)sysconf(_SC_PAGESIZE);
    char *data = mmap(NULL, page*2, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
    if (data == MAP_FAILED) return 1;
    memcpy(data, "procinsh mmap fixture", sizeof("procinsh mmap fixture"));
    if (mprotect(data+page, page, PROT_NONE)) return 1;
    ready(data); for (;;) sleep(1);
}
