#include "../src/space/ibs.c"
#include <assert.h>
static void put(struct ibs *s,uint64_t tail,const void *data,size_t size){for(size_t i=0;i<size;i++)((char*)s->map+s->map->data_offset)[(tail+i)%s->map->data_size]=((const char*)data)[i];s->map->data_tail=tail;s->map->data_head=tail+size;}
int main(void){
    size_t offset=sizeof(struct perf_event_mmap_page);struct ibs s={.fd=-1,.size=offset+128,.map=calloc(1,offset+128)};s.map->data_offset=offset;s.map->data_size=128;
    struct {struct perf_event_header header;uint32_t pid,tid;uint64_t time,addr,source;} record={{.type=PERF_RECORD_SAMPLE,.misc=PERF_RECORD_MISC_USER,.size=40},42,43,100,0x12345000,PERF_MEM_OP_LOAD};
    struct sample out[2];uint64_t lost=0;
    put(&s,110,&record,sizeof(record));assert(procinsh_ibs_poll(&s,out,2,&lost)==1);assert(out[0].pid==42&&out[0].tid==43&&out[0].addr==0x12345000&&out[0].time==100);assert(!lost);
    record.header.misc=PERF_RECORD_MISC_KERNEL;put(&s,0,&record,sizeof(record));assert(procinsh_ibs_poll(&s,out,2,&lost)==0);
    struct {struct perf_event_header header;uint64_t id,lost;} lr={{.type=PERF_RECORD_LOST,.size=24},1,7};put(&s,0,&lr,sizeof(lr));assert(procinsh_ibs_poll(&s,out,2,&lost)==0&&lost==7);
    record.header.size=0;put(&s,0,&record,sizeof(record));assert(procinsh_ibs_poll(&s,out,2,&lost)==0&&lost==8);
    free(s.map);return 0;
}
