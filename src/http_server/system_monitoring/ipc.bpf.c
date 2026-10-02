#include "vmlinux.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_core_read.h>
#include <bpf/bpf_tracing.h>
#include "pidns.bpf.h"
char LICENSE[] SEC("license") = "GPL";
struct event {
    __u64 time, start, inode, device, bytes;
    __u32 pid, kind, write, worker;
};
struct {
    __uint(type, BPF_MAP_TYPE_RINGBUF);
    __uint(max_entries, 8 * 1024 * 1024);
} events SEC(".maps");
struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __uint(max_entries, 1);
    __type(key, __u32);
    __type(value, __u64);
} lost SEC(".maps");
static __always_inline int emit(struct file *file, long ret, __u32 kind, __u32 write) {
    if (ret <= 0 || !file)
        return 0;
    struct task_struct *task = (void *)bpf_get_current_task_btf();
    __u32 pid = visible_tgid(task);
    if (!pid)
        return 0;
    struct inode *inode = BPF_CORE_READ(file, f_inode);
    if (!inode)
        return 0;
    struct event *e = bpf_ringbuf_reserve(&events, sizeof(*e), 0);
    if (!e) {
        __u32 zero = 0;
        __u64 *n = bpf_map_lookup_elem(&lost, &zero);
        if (n)
            __sync_fetch_and_add(n, 1);
        return 0;
    }
    e->time = bpf_ktime_get_ns();
    struct task_struct *leader = BPF_CORE_READ(task, group_leader);
    e->start = BPF_CORE_READ(leader, start_boottime);
    e->inode = BPF_CORE_READ(inode, i_ino);
    e->device = BPF_CORE_READ(inode, i_sb, s_dev);
    e->bytes = ret;
    e->pid = pid;
    e->kind = kind;
    e->write = write;
    e->worker = (BPF_CORE_READ(task, flags) & (0x00200000 | 0x00000010)) != 0;
    bpf_ringbuf_submit(e, 0);
    return 0;
}
#define PIPE(name, dir)                                                                            \
    SEC("fexit/" #name) int BPF_PROG(name, struct kiocb *iocb, struct iov_iter *iter, long ret) {  \
        return emit(BPF_CORE_READ(iocb, ki_filp), ret, 1, dir);                                    \
    }
PIPE(anon_pipe_read, 0)
PIPE(anon_pipe_write, 1)
// FIFO wrappers invoke anon_pipe_* too; attaching both would double count.
// Tracepoints also cover __sock_sendmsg's inlined userspace write/send paths.
SEC("tp_btf/sock_send_length") int BPF_PROG(send, struct sock *sk, int ret, int flags) {
    return emit(BPF_CORE_READ(sk, sk_socket, file), ret, 2, 1);
}
SEC("tp_btf/sock_recv_length") int BPF_PROG(recv, struct sock *sk, int ret, int flags) {
    if (flags & 2)
        return 0;
    return emit(BPF_CORE_READ(sk, sk_socket, file), ret, 2, 0);
}
