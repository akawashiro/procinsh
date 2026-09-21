#include "support.h"
int main(int argc, char **argv) {
    setup(argc, argv); char message[] = "procinsh memory fixture"; ready(message);
    for (;;) sleep(1);
}
