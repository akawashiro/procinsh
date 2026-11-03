#include "support.h"

#include <signal.h>
#include <sys/socket.h>
#include <sys/wait.h>

static pid_t peer;
static void cleanup(int sig) {
    (void)sig;
    if (peer > 0) {
        kill(peer, SIGTERM);
        waitpid(peer, NULL, 0);
    }
    _exit(0);
}

int main(int argc, char **argv) {
    setup(argc, argv);
    int pair[2], ready_pipe[2];
    if (socketpair(AF_UNIX, SOCK_STREAM, 0, pair) || pipe(ready_pipe))
        return 1;
    peer = fork();
    if (peer < 0)
        return 1;
    if (peer == 0) {
        prctl(PR_SET_PDEATHSIG, SIGTERM);
        if (getppid() == 1)
            return 1;
        close(STDIN_FILENO);
        close(ready_pipe[0]);
        if (dup2(pair[1], 61) < 0 || dup2(pair[0], 63) < 0)
            return 1;
        close(pair[0]);
        close(pair[1]);
        if (write(ready_pipe[1], "x", 1) != 1)
            return 1;
        close(ready_pipe[1]);
        for (;;)
            pause();
    }
    close(ready_pipe[1]);
    if (dup2(pair[0], 61) < 0 || dup2(pair[0], 63) < 0)
        return 1;
    close(pair[0]);
    close(pair[1]);
    signal(SIGTERM, cleanup);
    signal(SIGINT, cleanup);
    char byte;
    if (read(ready_pipe[0], &byte, 1) != 1)
        return 1;
    close(ready_pipe[0]);
    printf("%d %d\n", getpid(), peer);
    fflush(stdout);
    for (;;)
        pause();
}
