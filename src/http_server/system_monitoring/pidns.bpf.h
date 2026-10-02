#ifndef PROCINSH_PIDNS_H
#define PROCINSH_PIDNS_H

struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __uint(max_entries, 1);
    __type(key, __u32);
    __type(value, __u64);
} pid_namespace SEC(".maps");

// A TGID is the group leader's thread PID in the observer's namespace.
// Linux bounds PID namespace nesting to 32; ancestors can see descendants.
static __always_inline __u32 visible_tgid(struct task_struct *task) {
    __u32 zero = 0;
    __u64 *target = bpf_map_lookup_elem(&pid_namespace, &zero);
    if (!target || !*target)
        return 0;
    struct pid *pid = BPF_CORE_READ(task, group_leader, thread_pid);
    if (!pid)
        return 0;
    unsigned int level = BPF_CORE_READ(pid, level);
    if (level > 32)
        return 0;
    for (unsigned int i = 0; i < 33; i++) {
        if (i > level)
            break;
        struct upid number = {};
        bpf_core_read(&number, sizeof(number), &pid->numbers[i]);
        if (BPF_CORE_READ(number.ns, ns.inum) == *target)
            return number.nr;
    }
    return 0;
}
#endif
