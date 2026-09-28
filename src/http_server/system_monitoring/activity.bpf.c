#include "vmlinux.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_core_read.h>
#include <bpf/bpf_tracing.h>
char LICENSE[] SEC("license") = "GPL";
struct event { __u64 time, start, inode, device, bytes; __u32 pid, kind, write, worker; };
struct { __uint(type, BPF_MAP_TYPE_RINGBUF); __uint(max_entries, 8 * 1024 * 1024); } events SEC(".maps");
struct { __uint(type, BPF_MAP_TYPE_ARRAY); __uint(max_entries, 1); __type(key, __u32); __type(value, __u64); } lost SEC(".maps");
struct process_key { __u64 start; __u32 pid; __u32 pad; };
struct cpu_slot { struct process_key process; __u64 since; __u32 tid; __u32 pad; };
struct cpu_total { __u64 runtime; __u64 switches; };
struct { __uint(type, BPF_MAP_TYPE_PERCPU_ARRAY); __uint(max_entries, 1); __type(key, __u32); __type(value, struct cpu_slot); } cpu_current SEC(".maps");
struct { __uint(type, BPF_MAP_TYPE_LRU_PERCPU_HASH); __uint(max_entries, 65536); __type(key, struct process_key); __type(value, struct cpu_total); } cpu_totals SEC(".maps");

static __always_inline struct process_key process_key(struct task_struct *task) {
    struct task_struct *leader = BPF_CORE_READ(task, group_leader);
    struct process_key key = {
        .start = BPF_CORE_READ(leader, start_boottime),
        .pid = BPF_CORE_READ(leader, tgid),
    };
    return key;
}

SEC("tp_btf/sched_switch")
int BPF_PROG(schedule, bool preempt, struct task_struct *prev,
             struct task_struct *next, unsigned int prev_state) {
    __u32 zero = 0;
    __u64 now = bpf_ktime_get_ns();
    struct cpu_slot *slot = bpf_map_lookup_elem(&cpu_current, &zero);
    if (!slot)
        return 0;

    struct process_key previous = process_key(prev);
    if (previous.pid && slot->process.pid == previous.pid &&
        slot->process.start == previous.start && now >= slot->since) {
        struct cpu_total initial = {};
        struct cpu_total *total = bpf_map_lookup_elem(&cpu_totals, &previous);
        if (!total) {
            bpf_map_update_elem(&cpu_totals, &previous, &initial, BPF_NOEXIST);
            total = bpf_map_lookup_elem(&cpu_totals, &previous);
        }
        if (total) {
            total->runtime += now - slot->since;
            total->switches++;
        }
    }

    slot->process = process_key(next);
    slot->since = now;
    slot->tid = BPF_CORE_READ(next, pid);
    return 0;
}

SEC("tp_btf/sched_process_exit")
int BPF_PROG(process_exit, struct task_struct *task, bool group_dead) {
    if (group_dead) {
        struct process_key key = process_key(task);
        bpf_map_delete_elem(&cpu_totals, &key);
    }
    return 0;
}
static __always_inline int emit(struct file *file, long ret, __u32 kind, __u32 write) {
    if (ret <= 0 || !file) return 0;
    struct task_struct *task = (void *)bpf_get_current_task_btf();
    struct inode *inode = BPF_CORE_READ(file, f_inode);
    if (!inode) return 0;
    struct event *e = bpf_ringbuf_reserve(&events, sizeof(*e), 0);
    if (!e) { __u32 zero=0; __u64 *n=bpf_map_lookup_elem(&lost,&zero); if(n) __sync_fetch_and_add(n,1); return 0; }
    e->time=bpf_ktime_get_ns();
    struct task_struct *leader=BPF_CORE_READ(task, group_leader);
    e->start=BPF_CORE_READ(leader, start_boottime);
    e->inode=BPF_CORE_READ(inode, i_ino);
    e->device=BPF_CORE_READ(inode, i_sb, s_dev);
    e->bytes=ret; e->pid=bpf_get_current_pid_tgid()>>32; e->kind=kind; e->write=write;
    e->worker=(BPF_CORE_READ(task, flags) & (0x00200000 | 0x00000010)) != 0;
    bpf_ringbuf_submit(e,0); return 0;
}
#define PIPE(name, dir) SEC("fexit/" #name) int BPF_PROG(name, struct kiocb *iocb, struct iov_iter *iter, long ret) { return emit(BPF_CORE_READ(iocb,ki_filp),ret,1,dir); }
PIPE(anon_pipe_read,0)
PIPE(anon_pipe_write,1)
// FIFO wrappers invoke anon_pipe_* too; attaching both would double count.
// Tracepoints also cover __sock_sendmsg's inlined userspace write/send paths.
SEC("tp_btf/sock_send_length") int BPF_PROG(send, struct sock *sk, int ret, int flags) { return emit(BPF_CORE_READ(sk,sk_socket,file),ret,2,1); }
SEC("tp_btf/sock_recv_length") int BPF_PROG(recv, struct sock *sk, int ret, int flags) { if(flags & 2) return 0; return emit(BPF_CORE_READ(sk,sk_socket,file),ret,2,0); }
