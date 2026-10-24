#include "vmlinux.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_core_read.h>

char LICENSE[] SEC("license") = "GPL";

struct signal_event {
    __u64 timestamp_ns;
    __u64 src_start;
    __u64 dst_start;
    __u32 src_pid;
    __u32 dst_pid;
    __s32 signal;
    __u32 pad;
};
struct {
    __uint(type, BPF_MAP_TYPE_RINGBUF);
    __uint(max_entries, 256 * 1024);
} events SEC(".maps");
struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __uint(max_entries, 1);
    __type(key, __u32);
    __type(value, __u64);
} lost SEC(".maps");

/* The raw signal_generate tracepoint exposes the target task, including for
 * thread-directed signals. Both endpoints are normalized to group leaders. */
SEC("raw_tp/signal_generate")
int signal_generate(struct bpf_raw_tracepoint_args *ctx) {
    struct task_struct *src = (struct task_struct *)bpf_get_current_task();
    struct task_struct *dst = (struct task_struct *)ctx->args[2];
    src = BPF_CORE_READ(src, group_leader);
    dst = BPF_CORE_READ(dst, group_leader);
    struct signal_event *event = bpf_ringbuf_reserve(&events, sizeof(*event), 0);
    if (!event) {
        __u32 zero = 0;
        __u64 *count = bpf_map_lookup_elem(&lost, &zero);
        if (count)
            __sync_fetch_and_add(count, 1);
        return 0;
    }
    event->timestamp_ns = bpf_ktime_get_ns();
    event->src_start = BPF_CORE_READ(src, start_boottime);
    event->dst_start = BPF_CORE_READ(dst, start_boottime);
    event->src_pid = BPF_CORE_READ(src, tgid);
    event->dst_pid = BPF_CORE_READ(dst, tgid);
    event->signal = (__s32)ctx->args[0];
    event->pad = 0;
    bpf_ringbuf_submit(event, 0);
    return 0;
}
