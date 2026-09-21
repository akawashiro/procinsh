#include "support.h"
int main(int argc, char **argv) {
    setup(argc, argv); volatile uint64_t counter = 0; ready((void *)&counter);
    for (;;) counter++;
}
