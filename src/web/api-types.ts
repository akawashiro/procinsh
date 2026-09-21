// JSON contracts consumed by the UI. Keep these aligned with the Rust Serialize
// structs in process/, state/, snapshot/ and space/. Addresses stay hex strings.
export interface ProcessId {
  pid: number;
  start_time_ticks: number;
}
export interface ProcessSummary {
  identity: ProcessId;
  name: string;
  executable: string | null;
  command_line: string[] | null;
  uid: number | null;
  username: string | null;
  euid: number | null;
  effective_username: string | null;
  state: string;
  cpu_percent: number | null;
  rss_bytes: number;
  thread_count: number;
}
export interface MemoryMap {
  start: string;
  end: string;
  readable: boolean;
  writable: boolean;
  executable: boolean;
  private: boolean;
  permissions: string;
  file_offset: string;
  device: string;
  inode: number;
  pathname: string | null;
  rss_bytes: number | null;
  pss_bytes: number | null;
}
export interface ThreadObservation {
  tid: number;
  name: string;
  state: string;
  cpu: number;
  cpu_percent: number | null;
  priority: number;
  nice: number;
  scheduler: string;
  affinity: string | null;
  voluntary_context_switches: number | null;
  nonvoluntary_context_switches: number | null;
}
export interface HistoryPoint {
  timestamp: number;
  cpu_percent: number | null;
  rss_bytes: number;
  vms_bytes: number;
}
export interface ProcessObservation extends HistoryPoint {
  process_id: ProcessId;
  minor_faults: number;
  major_faults: number;
  voluntary_context_switches: number | null;
  nonvoluntary_context_switches: number | null;
  io: { read_bytes: number; write_bytes: number } | null;
  rates: Record<
    | "minor_faults"
    | "major_faults"
    | "voluntary_context_switches"
    | "nonvoluntary_context_switches"
    | "read_bytes"
    | "write_bytes",
    number | null
  >;
  threads: ThreadObservation[];
  cpu: number;
  nice: number;
  priority: number;
}
export interface Target {
  summary: ProcessSummary;
  observation: ProcessObservation | null;
  exited: boolean;
  error: string | null;
  maps: MemoryMap[];
  maps_captured_at: number | null;
  maps_error: string | null;
  rollup: {
    rss_bytes: number | null;
    pss_bytes: number | null;
    private_bytes: number | null;
  } | null;
  history: HistoryPoint[];
}
export interface ThreadSnapshot {
  tid: number;
  error: string | null;
  unwind_stop: string;
  registers: {
    name: string;
    value: string;
    decimal: string;
    kind: string;
    mapping: string | null;
    offset: string | null;
  }[];
  call_stack: {
    address: string;
    symbol: string | null;
    symbol_offset: string | null;
    source_file: string | null;
    line: number | null;
    inline_frames: {
      function: string | null;
      file: string | null;
      line: number | null;
    }[];
  }[];
  disassembly: {
    address: string;
    bytes: number[];
    error: string | null;
    instructions: {
      address: string;
      bytes: number[];
      text: string;
      current: boolean;
    }[];
  } | null;
}
export interface Capture {
  process_id: ProcessId;
  captured_at: number;
  paused_ms: number;
  threads: ThreadSnapshot[];
  maps: MemoryMap[];
}
export interface MemoryRead {
  process_id: ProcessId;
  address: string;
  requested_length: number;
  bytes: number[];
  partial: boolean;
  captured_at: number;
}
interface ProcessDetail {
  process_id: ProcessId;
  captured_at: number;
}
export interface Environment extends ProcessDetail {
  entries: { name: string; value: string | null }[];
  lossy_utf8: boolean;
}
export interface AuxVector extends ProcessDetail {
  word_bits: number;
  entries: {
    tag: string;
    name: string;
    value: string;
    decimal: string;
    kind: string;
    description: string;
    text: string | null;
    text_error: string | null;
  }[];
}
export interface DescriptorEndpoint {
  process_id: ProcessId;
  name: string;
  fd: number;
  access: string;
  relation: string;
}
export interface FileDescriptors extends ProcessDetail {
  warnings: string[];
  entries: {
    fd: number;
    kind: string;
    inode: string;
    target: string;
    access: string;
    protocol: string | null;
    state: string | null;
    local: string | null;
    remote: string | null;
    peer_inode: string | null;
    peers: DescriptorEndpoint[];
    holders: DescriptorEndpoint[];
    note: string;
  }[];
}
export interface SignalMask {
  hex: string;
  signals: string[];
}
interface SignalStatus {
  tid: number;
  name: string;
  pending: SignalMask;
  shared_pending: SignalMask;
  blocked: SignalMask;
  ignored: SignalMask;
  caught: SignalMask;
  queued: string;
}
export interface Signals extends ProcessDetail {
  leader: SignalStatus;
  threads: SignalStatus[];
  warnings: string[];
}
export interface DetailData {
  environment: Environment;
  auxv: AuxVector;
  fds: FileDescriptors;
  signals: Signals;
}
export interface SpaceNode {
  identity: ProcessId;
  parent_id: ProcessId | null;
  name: string;
  uid: number | null;
  username: string | null;
  euid: number | null;
  effective_username: string | null;
  cpu_percent: number | null;
  rss_bytes: number;
  maps: MemoryMap[];
  maps_epoch: number;
  maps_error: string | null;
}
export interface Port {
  process_id: ProcessId;
  fd: number;
  fd_count: number;
  resource: string;
  kind: string;
  access: number;
}
export interface SocketEndpoint {
  protocol: string;
  state: string;
  local: string | null;
  remote: string | null;
  network_peer: boolean;
  remote_hostname: string | null;
}
export interface Edge {
  id: string;
  a: Port;
  b: Port | null;
  label: string;
  socket: SocketEndpoint | null;
  candidate: boolean;
  shared: boolean;
}
export interface Topology {
  nodes: SpaceNode[];
  edges: Edge[];
}
export interface IoActivity {
  process_id: ProcessId;
  resource: string;
  write: boolean;
  bytes: number;
  count: number;
}
export interface FileActivity extends IoActivity {
  path: string | null;
}
export interface CpuActivity {
  process_id: ProcessId;
  runtime_ns: number;
  switches: number;
  running_threads: number;
  cpus: number[];
}
export interface SpaceActivity {
  window_ms: number;
  invalidated?: number[];
  status?: { memory?: string };
  files?: FileActivity[];
  cpu?: CpuActivity[];
  ipc?: IoActivity[];
  memory?: {
    process_id: ProcessId;
    maps_epoch: number;
    page: string;
    mode: string;
    count: number;
  }[];
}
export interface Lease {
  token: string;
}
