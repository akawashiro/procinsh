#include "support.h"
#include <sys/socket.h>
#include <netinet/in.h>
#include <signal.h>
#include <sys/wait.h>

static pid_t peer;
static void cleanup(int sig) {
    (void)sig;
    if (peer > 0) { kill(peer, SIGTERM); waitpid(peer, NULL, 0); }
    _exit(0);
}
static int bound_socket(int type) {
    int fd = socket(AF_INET, type, 0);
    struct sockaddr_in addr = {.sin_family=AF_INET, .sin_addr.s_addr=htonl(INADDR_LOOPBACK)};
    if (fd < 0 || bind(fd, (struct sockaddr *)&addr, sizeof(addr))) { perror("socket/bind"); exit(1); }
    return fd;
}
static void connect_to(int fd, int other) {
    struct sockaddr_in addr; socklen_t len=sizeof(addr);
    if (getsockname(other,(struct sockaddr *)&addr,&len) || connect(fd,(struct sockaddr *)&addr,len)) { perror("connect");exit(1); }
}
int main(int argc, char **argv) {
    setup(argc,argv);
    int pipes[2], unix_pair[2];
    if (pipe(pipes) || socketpair(AF_UNIX,SOCK_STREAM,0,unix_pair)) return 1;
    int listener=bound_socket(SOCK_STREAM); if (listen(listener,4)) return 1;
    int client=socket(AF_INET,SOCK_STREAM,0); connect_to(client,listener);
    int accepted=accept(listener,NULL,NULL); if (accepted<0) return 1;
    int udp0=bound_socket(SOCK_DGRAM),udp1=bound_socket(SOCK_DGRAM);
    connect_to(udp0,udp1); connect_to(udp1,udp0);
    signal(SIGCHLD,SIG_IGN);
    peer=fork(); if (peer<0) return 1;
    if (peer==0) {
        prctl(PR_SET_PDEATHSIG,SIGTERM); if (getppid()==1) return 0;
        prctl(PR_SET_NAME,"procinsh-ipc-peer");
        dup2(pipes[0],60); dup2(unix_pair[1],61); dup2(accepted,62); dup2(unix_pair[0],63); dup2(udp1,64);
    } else {
        signal(SIGTERM,cleanup); signal(SIGINT,cleanup); signal(SIGALRM,cleanup);
        prctl(PR_SET_NAME,"procinsh-ipc");
        dup2(pipes[1],60); dup2(unix_pair[0],61); dup2(client,62); dup2(unix_pair[0],63); dup2(udp0,64); dup2(listener,65);
    }
    close(pipes[0]);close(pipes[1]);close(unix_pair[0]);close(unix_pair[1]);close(listener);close(client);close(accepted);close(udp0);close(udp1);
    if (peer>0) { printf("%d 0x0 %d\n",getpid(),peer);fflush(stdout); }
    for (;;) sleep(1);
}
