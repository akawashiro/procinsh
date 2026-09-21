#define _GNU_SOURCE
#include <sys/socket.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <sys/prctl.h>
#include <netinet/in.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
#include <signal.h>
#include <time.h>
#include <stdint.h>
static void check(int n){if(n<0){perror("activity");exit(1);}}
int main(void){
    alarm(45);prctl(PR_SET_NAME,"alpha-activity");
    int p[2],u[2],tcp[2],udp[2];check(pipe(p));check(socketpair(AF_UNIX,SOCK_STREAM,0,u));
    int listener=socket(AF_INET,SOCK_STREAM,0);check(listener);struct sockaddr_in addr={.sin_family=AF_INET,.sin_addr.s_addr=htonl(INADDR_LOOPBACK)};socklen_t len=sizeof(addr);check(bind(listener,(void*)&addr,len));check(listen(listener,1));check(getsockname(listener,(void*)&addr,&len));tcp[0]=socket(AF_INET,SOCK_STREAM,0);check(connect(tcp[0],(void*)&addr,len));tcp[1]=accept(listener,NULL,NULL);check(tcp[1]);close(listener);
    struct sockaddr_in ua[2];for(int i=0;i<2;i++){udp[i]=socket(AF_INET,SOCK_DGRAM,0);check(udp[i]);ua[i]=(struct sockaddr_in){.sin_family=AF_INET,.sin_addr.s_addr=htonl(INADDR_LOOPBACK)};check(bind(udp[i],(void*)&ua[i],len));check(getsockname(udp[i],(void*)&ua[i],&len));}for(int i=0;i<2;i++)check(connect(udp[i],(void*)&ua[1-i],len));
    pid_t child=fork();check(child);
    if(!child){prctl(PR_SET_PDEATHSIG,SIGTERM);if(getppid()==1)return 1;prctl(PR_SET_NAME,"alpha-activity-peer");close(p[1]);close(u[0]);close(tcp[0]);close(udp[0]);char buf[256];int fds[]={p[0],u[1],tcp[1],udp[1]};for(int i=0;i<4;i++){size_t n=0;while(n<sizeof(buf)){int r=read(fds[i],buf+n,sizeof(buf)-n);if(r<=0)return 2;n+=r;}}for(int i=1;i<4;i++){if(recv(fds[i],buf,128,MSG_PEEK|MSG_WAITALL)!=128)return 4;if(recv(fds[i],buf,128,MSG_WAITALL)!=128)return 5;}sleep(10);return 0;}
    close(p[0]);close(u[1]);close(tcp[1]);close(udp[1]);
    size_t size=64*1024*1024;volatile uint64_t *memory=mmap(NULL,size,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANONYMOUS,-1,0);if(memory==MAP_FAILED)return 2;
    printf("%d %d %p %zu\n",getpid(),child,(void*)memory,size);fflush(stdout);if(getchar()==EOF){kill(child,SIGTERM);return 0;}
    char buf[256]={0};int fds[]={p[1],u[0],tcp[0],udp[0]};for(int i=0;i<4;i++)if(write(fds[i],buf,sizeof(buf))!=sizeof(buf))return 3;
    for(int i=1;i<4;i++)if(send(fds[i],buf,128,0)!=128)return 6;
    // A failed send must not contribute bytes or operations.
    if(send(u[0],NULL,16,0)>=0)return 7;
    struct timespec begin,now;clock_gettime(CLOCK_MONOTONIC,&begin);uint64_t sum=0;
    do{for(size_t i=0;i<size/8;i+=8){memory[i]=i+sum;sum+=memory[i];}clock_gettime(CLOCK_MONOTONIC,&now);}while(now.tv_sec-begin.tv_sec<4);
    printf("done %lu\n",sum);fflush(stdout);sleep(5);kill(child,SIGTERM);waitpid(child,NULL,0);return 0;
}
