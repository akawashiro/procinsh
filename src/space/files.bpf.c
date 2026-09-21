#include "vmlinux.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_core_read.h>
#include <bpf/bpf_tracing.h>
char LICENSE[] SEC("license") = "GPL";

struct file_event {
    __u64 start, inode, device, bytes;
    __u32 pid, write, path_len, generation;
    char path[4096];
};
struct pending_io { struct file_event event; __u64 file; __u32 depth; };
struct { __uint(type, BPF_MAP_TYPE_HASH); __uint(max_entries, 4096); __type(key, __u64); __type(value, struct pending_io); } pending SEC(".maps");
struct { __uint(type, BPF_MAP_TYPE_RINGBUF); __uint(max_entries, 8 * 1024 * 1024); } events SEC(".maps");
struct { __uint(type, BPF_MAP_TYPE_ARRAY); __uint(max_entries, 1); __type(key, __u32); __type(value, __u64); } lost SEC(".maps");
static const struct pending_io empty = {};
static __always_inline void drop(void) {
    __u32 zero = 0;
    __u64 *n = bpf_map_lookup_elem(&lost, &zero);
    if (n) __sync_fetch_and_add(n, 1);
}
static __always_inline int begin(struct file *file, __u32 write) {
    __u64 tid = bpf_get_current_pid_tgid();
    struct pending_io *p = bpf_map_lookup_elem(&pending, &tid);
    // Filesystems may call back into VFS: only count the outer operation.
    if (p) { p->depth++; return 0; }
    struct task_struct *task = (void *)bpf_get_current_task_btf();
    if (!file || (BPF_CORE_READ(task, flags) & (0x00200000 | 0x00000010))) return 0;
    struct inode *inode = BPF_CORE_READ(file, f_inode);
    if (!inode || (BPF_CORE_READ(inode, i_mode) & 0170000) != 0100000) return 0;
    if (bpf_map_update_elem(&pending, &tid, &empty, BPF_NOEXIST)) { drop(); return 0; }
    p = bpf_map_lookup_elem(&pending, &tid);
    if (!p) return 0;
    p->file = (__u64)file;
    p->depth = 1;
    p->event.start = BPF_CORE_READ(task, group_leader, start_boottime);
    p->event.pid = tid >> 32;
    p->event.inode = BPF_CORE_READ(inode, i_ino);
    p->event.device = BPF_CORE_READ(inode, i_sb, s_dev);
    p->event.generation = BPF_CORE_READ(inode, i_generation);
    p->event.write = write;
    return 0;
}
// bpf_d_path is permitted at security_file_permission, not at vfs_read/write.
// Capture while the caller owns the file, before a close or FD reuse is possible.
SEC("fentry/security_file_permission")
int BPF_PROG(file_path, struct file *file, int mask) {
    __u64 tid = bpf_get_current_pid_tgid();
    struct pending_io *p = bpf_map_lookup_elem(&pending, &tid);
    if (!p || p->file != (__u64)file || p->depth != 1) return 0;
    long len = bpf_d_path(&file->f_path, p->event.path, sizeof(p->event.path));
    p->event.path_len = len > 0 && len <= sizeof(p->event.path) ? len : 0;
    return 0;
}
static __always_inline int finish(long ret) {
    __u64 tid = bpf_get_current_pid_tgid();
    struct pending_io *p = bpf_map_lookup_elem(&pending, &tid);
    if (!p) return 0;
    if (p->depth > 1) { p->depth--; return 0; }
    if (ret > 0) {
        p->event.bytes = ret;
        if (bpf_ringbuf_output(&events, &p->event, sizeof(p->event), 0)) drop();
    }
    bpf_map_delete_elem(&pending, &tid);
    return 0;
}
#define SCALAR(name, dir) \
SEC("fentry/" #name) int BPF_PROG(enter_##name, struct file *file) { return begin(file, dir); } \
SEC("fexit/" #name) int BPF_PROG(exit_##name, struct file *file, void *buf, size_t count, loff_t *pos, long ret) { return finish(ret); }
#define VECTOR(name, dir) \
SEC("fentry/" #name) int BPF_PROG(enter_##name, struct file *file) { return begin(file, dir); } \
SEC("fexit/" #name) int BPF_PROG(exit_##name, struct file *file, const struct iovec *vec, unsigned long vlen, loff_t *pos, rwf_t flags, long ret) { return finish(ret); }
SCALAR(vfs_read, 0)
SCALAR(vfs_write, 1)
VECTOR(vfs_readv, 0)
VECTOR(vfs_writev, 1)
SEC("tp_btf/sched_process_exit")
int BPF_PROG(file_exit, struct task_struct *task, bool group_dead) {
    __u64 tid = bpf_get_current_pid_tgid();
    bpf_map_delete_elem(&pending, &tid);
    return 0;
}
