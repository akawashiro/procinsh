export type SensorState = {state:"idle" | "starting" | "observing"} | {state:"unavailable" | "error";message:string};
export interface SystemMonitorStatus {
  active:boolean; ipc:SensorState; cpu:SensorState; files:SensorState;
  coverage?:string; files_coverage?:string; lost?:number; unresolved?:number; files_lost?:number;
}
export interface MappingPermissions { readable: boolean; writable: boolean; executable: boolean; private: boolean }
export interface RegisterMapping extends MappingPermissions { pathname: string | null }
export type MemoryKind = "integer" | "stack" | "heap" | "shared_library" | "executable" | "file" | "anonymous";
export interface CpuRange { start: number; end: number }
export type SchedulerPolicy = {kind:"other" | "fifo" | "rr" | "batch" | "idle" | "deadline" | "ext"} | {kind:"unknown";code:number};
export interface Signal { number: number; name: string }
export interface SignalQueue { count: string; limit: string }
export type FdAccess = "read" | "write" | "read_write" | "unknown";
export type FdKind = "pipe" | "socket" | "fifo";
export interface InetAddress { ip: string; port: number }
export type SocketType = {kind: "stream" | "dgram" | "seqpacket"} | {kind:"unknown";code:number};
export type SocketProtocol = {kind:"tcp" | "udp";family:"ipv4" | "ipv6"} | {kind:"unix";socket_type:SocketType};
export type SocketState = {kind:"established" | "syn_sent" | "syn_recv" | "fin_wait1" | "fin_wait2" | "time_wait" | "close" | "close_wait" | "last_ack" | "listen" | "closing" | "new_syn_recv" | "unconnected" | "connecting" | "connected" | "disconnecting"} | {kind:"unknown_inet" | "unknown_unix";code:number};
export interface DeviceId { major: number; minor: number }
export interface FileIdentity { device: DeviceId; inode: string; generation: number }
export interface IpcIdentity { kind: "pipe" | "socket"; device: DeviceId; inode: string }
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
  file_offset: string;
  device: DeviceId;
  inode: string;
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
  scheduler: SchedulerPolicy;
  affinity: CpuRange[] | null;
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
    kind: MemoryKind;
    mapping: RegisterMapping | null;
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
  access: FdAccess;
  relation: string;
}
export interface FileDescriptors extends ProcessDetail {
  warnings: string[];
  entries: {
    fd: number;
    kind: FdKind;
    inode: string;
    target: string;
    access: FdAccess;
    protocol: SocketProtocol | null;
    state: SocketState | null;
    local: InetAddress | null;
    remote: InetAddress | null;
    path: string | null;
    peer_inode: string | null;
    peers: DescriptorEndpoint[];
    holders: DescriptorEndpoint[];
    note: string;
  }[];
}
export interface SignalMask {
  hex: string;
  signals: Signal[];
}
interface SignalStatus {
  tid: number;
  name: string;
  pending: SignalMask;
  shared_pending: SignalMask;
  blocked: SignalMask;
  ignored: SignalMask;
  caught: SignalMask;
  queued: SignalQueue;
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
export interface Process {
  identity: ProcessId;
  parent_id: ProcessId | null;
  name: string;
  uid: number | null;
  username: string | null;
  euid: number | null;
  effective_username: string | null;
  maps: MemoryMap[];
  maps_epoch: number;
  maps_error: string | null;
}
export interface FdEndpoint {
  process_id: ProcessId;
  fd: number;
  fd_count: number;
  resource: IpcIdentity;
  kind: FdKind;
  access: FdAccess;
}
export interface SocketEndpoint {
  protocol: SocketProtocol;
  state: SocketState;
  local: InetAddress | null;
  remote: InetAddress | null;
  path: string | null;
  network_peer: boolean;
  remote_hostname: string | null;
}
export interface FdRelation {
  id: string;
  endpoint: FdEndpoint;
  peer: FdEndpoint | null;
  label: string;
  socket: SocketEndpoint | null;
  candidate: boolean;
  shared: boolean;
}
export interface SystemSnapshot {
  processes: Process[];
  fd_relations: FdRelation[];
}
export interface IoActivity {
  process_id: ProcessId;
  resource: IpcIdentity;
  write: boolean;
  bytes: number;
  count: number;
}
export interface FileActivity {
  process_id: ProcessId; file: FileIdentity; path: string | null;
  write: boolean; bytes: number; count: number;
}
export interface CpuActivity {
  process_id: ProcessId;
  runtime_ns: number;
  switches: number;
  running_threads: number;
  cpus: number[];
}
export interface SpaceActivity {
  status: SystemMonitorStatus;
  window_ms: number;
  files?: FileActivity[];
  cpu?: CpuActivity[];
  ipc?: IoActivity[];
}
