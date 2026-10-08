import type {
  CpuActivity,
  FdEndpoint,
  FdRelation,
  MemoryMap,
  Process,
  ProcessObservation,
  ProcessSummary,
  SocketEndpoint,
  SpaceActivity,
  Target,
  ThreadObservation,
  ThreadSample,
} from "../../src/shared/api-types.js";

// Complete API values let each regression fixture specify only the fields it exercises.
export function processSummary(
  extra: Partial<ProcessSummary> = {},
): ProcessSummary {
  return {
    identity: { pid: 1, start_time_ticks: 1 },
    name: "process",
    executable: null,
    command_line: null,
    uid: null,
    username: null,
    euid: null,
    effective_username: null,
    state: "S",
    cpu_percent: null,
    rss_bytes: 0,
    thread_count: 1,
    started_at: null,
    ...extra,
  };
}
export function processInfo(extra: Partial<Process> = {}): Process {
  return {
    identity: { pid: 1, start_time_ticks: 1 },
    parent_id: null,
    name: "process",
    uid: null,
    username: null,
    euid: null,
    effective_username: null,
    maps: [],
    maps_epoch: 0,
    maps_error: null,
    ...extra,
  };
}
export function memoryMap(extra: Partial<MemoryMap> = {}): MemoryMap {
  return {
    start: "0x1000",
    end: "0x2000",
    readable: true,
    writable: false,
    executable: false,
    private: true,
    file_offset: "0x0",
    device: { major: 0, minor: 0 },
    inode: "0",
    pathname: null,
    ...extra,
  };
}
export function fdEndpoint(extra: Partial<FdEndpoint> = {}): FdEndpoint {
  return {
    process_id: { pid: 1, start_time_ticks: 1 },
    fd: 3,
    fd_count: 1,
    resource: { kind: "pipe", device: { major: 0, minor: 1 }, inode: "2" },
    kind: "pipe",
    access: "read_write",
    ...extra,
  };
}
export function socketEndpoint(
  extra: Partial<SocketEndpoint> = {},
): SocketEndpoint {
  return {
    protocol: { kind: "tcp", family: "ipv4" },
    state: { kind: "established" },
    local: null,
    remote: null,
    path: null,
    network_peer: false,
    remote_hostname: null,
    ...extra,
  };
}
export function fdRelation(extra: Partial<FdRelation> = {}): FdRelation {
  return {
    id: "pipe",
    endpoint: fdEndpoint(),
    peer: null,
    label: "pipe",
    socket: null,
    candidate: false,
    shared: false,
    ...extra,
  };
}
export function spaceActivity(
  extra: Partial<SpaceActivity> = {},
): SpaceActivity {
  return {
    status: {
      active: true,
      ipc: { state: "observing" },
      cpu: { state: "observing" },
      files: { state: "observing" },
    },
    window_ms: 100,
    ...extra,
  };
}
export function cpuActivity(extra: Partial<CpuActivity> = {}): CpuActivity {
  return {
    process_id: { pid: 1, start_time_ticks: 1 },
    runtime_ns: 0,
    switches: 0,
    running_threads: 0,
    cpus: [],
    ...extra,
  };
}
export function threadSample(extra: Partial<ThreadSample> = {}): ThreadSample {
  return {
    tid: 1,
    sampled_at: null,
    sample_age_ms: null,
    cpu: null,
    lost_samples: 0,
    error: null,
    unwind_stop: "",
    registers: [],
    call_stack: [],
    disassembly: null,
    ...extra,
  };
}
export function threadObservation(
  extra: Partial<ThreadObservation> = {},
): ThreadObservation {
  return {
    tid: 1,
    name: "thread",
    state: "S",
    cpu: 0,
    cpu_percent: null,
    priority: 0,
    nice: 0,
    scheduler: { kind: "other" },
    affinity: null,
    voluntary_context_switches: null,
    nonvoluntary_context_switches: null,
    ...extra,
  };
}
export function processObservation(
  extra: Partial<ProcessObservation> = {},
): ProcessObservation {
  return {
    process_id: { pid: 1, start_time_ticks: 1 },
    timestamp: 0,
    cpu_percent: null,
    rss_bytes: 0,
    vms_bytes: 0,
    minor_faults: 0,
    major_faults: 0,
    voluntary_context_switches: null,
    nonvoluntary_context_switches: null,
    io: null,
    rates: {
      minor_faults: null,
      major_faults: null,
      voluntary_context_switches: null,
      nonvoluntary_context_switches: null,
      read_bytes: null,
      write_bytes: null,
    },
    threads: [],
    cpu: 0,
    nice: 0,
    priority: 0,
    ...extra,
  };
}
export function targetInfo(extra: Partial<Target> = {}): Target {
  return {
    summary: processSummary(),
    observation: null,
    exited: false,
    error: null,
    maps: [],
    maps_captured_at: null,
    maps_error: null,
    rollup: null,
    history: [],
    live_samples: [],
    sampling_error: null,
    ...extra,
  };
}
