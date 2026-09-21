#define _GNU_SOURCE
#include <linux/perf_event.h>
#include <sys/syscall.h>
#include <sys/mman.h>
#include <sys/ioctl.h>
#include <unistd.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <errno.h>
#include <time.h>
struct ibs { int fd; size_t size; struct perf_event_mmap_page *map; };
struct sample { uint64_t time, addr, source; uint32_t pid, tid; };
void *procinsh_ibs_open(int tid, int type, uint64_t period) {
    struct perf_event_attr attr={0}; attr.type=type; attr.size=sizeof(attr);
    attr.sample_period=period; attr.sample_type=PERF_SAMPLE_TID|PERF_SAMPLE_TIME|PERF_SAMPLE_ADDR|PERF_SAMPLE_DATA_SRC;
    attr.use_clockid=1;attr.clockid=CLOCK_MONOTONIC;
    attr.disabled=1; attr.sample_id_all=1; attr.wakeup_events=1;
    attr.mmap=1;attr.mmap2=1;attr.mmap_data=1;attr.comm=1;attr.comm_exec=1;
    // IBS does not support exclude_kernel: discard non-user addresses/PIDs in userspace.
    int fd=syscall(__NR_perf_event_open,&attr,tid,-1,-1,PERF_FLAG_FD_CLOEXEC);
    if(fd<0) return NULL;
    size_t size=(1+64)*sysconf(_SC_PAGESIZE);
    void *map=mmap(NULL,size,PROT_READ|PROT_WRITE,MAP_SHARED,fd,0);
    if(map==MAP_FAILED) { int e=errno; close(fd); errno=e; return NULL; }
    struct ibs *s=calloc(1,sizeof(*s));
    if(!s) { munmap(map,size); close(fd); return NULL; }
    s->fd=fd; s->size=size; s->map=map;
    if(ioctl(fd,PERF_EVENT_IOC_ENABLE,0)<0) {int e=errno; munmap(map,size);close(fd);free(s);errno=e;return NULL;}
    return s;
}
void procinsh_ibs_close(void *p) { struct ibs *s=p; if(!s)return; close(s->fd); munmap(s->map,s->size); free(s); }
static void copy_ring(struct ibs *s,uint64_t off,void *dst,size_t len) {
    size_t size=s->map->data_size; size_t start=off%size; size_t first=size-start; if(first>len) first=len;
    char *base=(char*)s->map+s->map->data_offset;
    memcpy(dst,base+start,first); if(first<len)memcpy((char*)dst+first,base,len-first);
}
int procinsh_ibs_poll(void *p,struct sample *out,int cap,uint64_t *lost) {
    struct ibs *s=p; uint64_t head=__atomic_load_n(&s->map->data_head,__ATOMIC_ACQUIRE),tail=s->map->data_tail; int n=0;
    if(head-tail>s->map->data_size){(*lost)++;tail=head;}
    while(tail<head && n<cap) {
        struct perf_event_header h; copy_ring(s,tail,&h,sizeof(h));
        if(h.size<sizeof(h)||h.size>s->map->data_size||tail+h.size>head){(*lost)++;tail=head;break;}
        if(h.type==PERF_RECORD_SAMPLE && h.size>=40 && (h.misc & PERF_RECORD_MISC_CPUMODE_MASK)==PERF_RECORD_MISC_USER) {
            uint64_t data[4];copy_ring(s,tail+8,data,sizeof(data));
            out[n++]=(struct sample){.pid=(uint32_t)data[0],.tid=data[0]>>32,.time=data[1],.addr=data[2],.source=data[3]};
        } else if((h.type==PERF_RECORD_MMAP2 || (h.type==PERF_RECORD_COMM && (h.misc & PERF_RECORD_MISC_COMM_EXEC))) && h.size>=32) {
            uint32_t pid;uint64_t time;copy_ring(s,tail+8,&pid,4);copy_ring(s,tail+h.size-8,&time,8);
            out[n++]=(struct sample){.pid=pid,.time=time,.source=UINT64_MAX};
        } else if(h.type==PERF_RECORD_LOST && h.size>=24) {uint64_t v;copy_ring(s,tail+16,&v,8);*lost+=v;}
        else if(h.type==PERF_RECORD_LOST_SAMPLES && h.size>=16) {uint64_t v;copy_ring(s,tail+8,&v,8);*lost+=v;}
        tail+=h.size;
    }
    __atomic_store_n(&s->map->data_tail,tail,__ATOMIC_RELEASE);return n;
}

int procinsh_ibs_period(void *p,uint64_t period){return ioctl(((struct ibs*)p)->fd,PERF_EVENT_IOC_PERIOD,&period);}
