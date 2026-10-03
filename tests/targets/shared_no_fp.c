#include "support.h"

void shared_recurse(int depth);
int main(int argc, char **argv) {
    setup(argc, argv);
    ready(NULL);
    for (;;)
        shared_recurse(12);
}
