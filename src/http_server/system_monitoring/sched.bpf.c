#include "vmlinux.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_core_read.h>
#include <bpf/bpf_tracing.h>
char LICENSE[] SEC("license") = "GPL";
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
