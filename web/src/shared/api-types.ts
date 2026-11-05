export type SensorState =

  | { state: "idle" | "observing" }
  | { state: "unavailable" | "error"; message: string };
export interface SystemMonitorStatus {
  active: boolean;
  ipc: SensorState;
  cpu: SensorState;
  files: SensorState;
  signals?: SensorState;
  signals_lost?: number;
  coverage?: string;
  files_coverage?: string;
  lost?: number;
  unresolved?: number;
  files_lost?: number;
}
export interface MappingPermissions {
  readable: boolean;
  writable: boolean;
  executable: boolean;
  private: boolean;
}
export interface RegisterMapping extends MappingPermissions {
  pathname: string | null;
}
export type MemoryKind =
  | "integer"
  | "stack"
  | "heap"
  | "shared_library"
  | "executable"
  | "file"
  | "anonymous";
export interface CpuRange {
  start: number;
  end: number;
}
export type SchedulerPolicy =
  | { kind: "other" | "fifo" | "rr" | "batch" | "idle" | "deadline" | "ext" }
  | { kind: "unknown"; code: number };
export type FdAccess = "read" | "write" | "read_write" | "unknown";
export type FdKind = "pipe" | "socket" | "fifo";
export interface InetAddress {
  ip: string;
  port: number;
}
export type SocketType =
  | {
      kind:
        "stream" | "dgram" | "raw" | "rdm" | "seqpacket" | "dccp" | "packet";
    }
  | { kind: "unknown"; code: number };
export type SocketProtocol =
  | { kind: "tcp" | "udp"; family: "ipv4" | "ipv6" }
  | { kind: "unix"; socket_type: SocketType }
  | {
      kind: "other";
      family: number;
      socket_type: SocketType | null;
      protocol: number;
    };
export type SocketState =
  | {
      kind:
        | "established"
        | "syn_sent"
        | "syn_recv"
        | "fin_wait1"
        | "fin_wait2"
        | "time_wait"
        | "close"
        | "close_wait"
        | "last_ack"
        | "listen"
        | "closing"
        | "new_syn_recv"
        | "unconnected"
        | "connecting"
        | "connected"
        | "disconnecting";
    }
  | { kind: "unknown_inet" | "unknown_unix"; code: number };
export interface DeviceId {
  major: number;
  minor: number;
}
export interface FileIdentity {
  device: DeviceId;
  inode: string;
  generation: number;
}
export interface IpcIdentity {
  kind: "pipe" | "socket";
  device: DeviceId;
  inode: string;
}
// JSON contracts consumed by the UI. Keep these aligned with the Rust Serialize
// structs in process/, state/, snapshot/ and space/. Addresses stay hex strings.
export interface ProcessId {
  pid: number;
  start_time_ticks: number;
  /** Set by the browser for a process of a remote host; the server never sends it. */
  host?: string;
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
  started_at: number | null;
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
}
export interface SmapsEntry extends MemoryMap {
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
  maps: SmapsEntry[];
  maps_captured_at: number | null;
  maps_error: string | null;
  rollup: {
    rss_bytes: number | null;
    pss_bytes: number | null;
    private_bytes: number | null;
  } | null;
  history: HistoryPoint[];
  live_samples: ThreadSample[];
  sampling_error: string | null;
}
/** Instruction set of the inspected process. */
export type Architecture = "x86_64" | "aarch64";
/** Architecture-independent register purpose; `kind` describes the value's mapping. */
export type RegisterRole = "pc" | "sp" | "fp" | "lr" | "flags" | "general";
export interface ThreadSample {
  /** `null` when the target's instruction set is unknown or unsupported. */
  architecture: Architecture | null;
  sampled_at: number | null;
  sample_age_ms: number | null;
  cpu: number | null;
  lost_samples: number;
  tid: number;
  error: string | null;
  unwind_stop: string;
  registers: {
    name: string;
    role: RegisterRole;
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
    connection_status?: SocketConnectionStatus | null;
    local: InetAddress | null;
    remote: InetAddress | null;
    path: string | null;
    peer_inode: string | null;
    peers: DescriptorEndpoint[];
    holders: DescriptorEndpoint[];
    note: string;
  }[];
}
export interface DetailData {
  environment: Environment;
  auxv: AuxVector;
  fds: FileDescriptors;
}
export interface Process {
  identity: ProcessId;
  parent_id: ProcessId | null;
  name: string;
  command_line: string[] | null;
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
  /** Absent on older servers; null for non-socket descriptors. */
  connection_status?: SocketConnectionStatus | null;
}
export type SocketConnectionStatus =
  "listening" | "unconnected" | "peer_present" | "unknown";
export interface SocketEndpoint {
  protocol: SocketProtocol;
  /** Null when only the kind was observed. Peer identification is independent. */
  state: SocketState | null;
  local: InetAddress | null;
  remote: InetAddress | null;
  path: string | null;
  network_peer: boolean;
  remote_hostname: string | null;
  /** The remote InfiniBand port GID of an IPoIB destination; absent on older servers. */
  remote_gid?: string | null;
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
export type HardwareRole =
  "host_bridge" | "root_port" | "switch_port" | "endpoint";
export type HardwareKind = "gpu" | "rdma" | "accelerator" | "nv_switch";
export interface PcieLink {
  speed_gts: number;
  width: number;
}
export interface HardwareDevice {
  /** A PCI function address such as `0000:85:00.0`, or a host bridge such as `pci0000:80`. */
  address: string;
  parent: string | null;
  role: HardwareRole;
  kind: HardwareKind | null;
  label: string;
  class: string | null;
  vendor_id: string | null;
  device_id: string | null;
  driver: string | null;
  numa_node: number | null;
  /** Null for host bridges and runtime-suspended devices. */
  link: { current: PcieLink | null; max: PcieLink | null } | null;
  power_state: string | null;
  runtime_status: string | null;
  iommu_group: number | null;
  bars: { index: number; bytes: number; prefetchable: boolean }[];
  gpu: {
    vendor: string;
    model: string | null;
    uuid: string | null;
    minor: number | null;
    drm: string[];
  } | null;
  rdma: { name: string; hca_type: string | null; fw_ver: string | null }[];
  netdevs: string[];
}
/** Like `nvidia-smi topo -m`, from the closest to the farthest. */
export type PathClass = "pix" | "pxb" | "phb" | "node" | "sys";
export interface HardwarePath {
  id: string;
  a: string;
  b: string;
  class: PathClass;
  /** Nodes between the ends, in order from `a` to `b`. */
  via: string[];
  bottleneck: PcieLink | null;
}
export type DeviceHandleKind =
  "nvidia" | "drm_render" | "drm_card" | "uverbs" | "umad" | "accel";
/** A process's descriptors for one device and kind. */
export interface DeviceHandle {
  id: string;
  process_id: ProcessId;
  device: string;
  kind: DeviceHandleKind;
  fd: number;
  fd_count: number;
}
/** A kernel enumeration: a known `kind`, or `unknown` with its numeric code. */
export type Coded<K extends string> =
  { kind: K; code?: never } | { kind: "unknown"; code: number };
/** `enum ib_qp_type`; unknown codes are preserved. */
export type RdmaQpType = Coded<
  | "smi"
  | "gsi"
  | "rc"
  | "uc"
  | "ud"
  | "raw_ipv6"
  | "raw_ethertype"
  | "raw_packet"
  | "xrc_ini"
  | "xrc_tgt"
  | "driver"
>;
export type RdmaQpState = Coded<
  "reset" | "init" | "rtr" | "rts" | "sqd" | "sqe" | "err"
>;
export type RdmaCmState = Coded<
  | "idle"
  | "addr_query"
  | "addr_resolved"
  | "route_query"
  | "route_resolved"
  | "connect"
  | "disconnect"
  | "addr_bound"
  | "listen"
  | "device_removal"
  | "destroying"
>;
export type RdmaPortSpace = Coded<"ipoib" | "tcp" | "udp" | "ib">;
/** A connection manager address; the IB service ID is a 64-bit hex string. */
export type RdmaAddress =
  | { kind: "inet"; ip: string; port: number }
  | { kind: "ib"; gid: string; service_id: string; pkey: number }
  | { kind: "unknown"; family: number };
export type RdmaEndpointOwner =
  | { kind: "process"; process_id: ProcessId }
  | { kind: "kernel"; name: string }
  | { kind: "unknown"; pid: number | null };
export interface RdmaEndpoint {
  owner: RdmaEndpointOwner;
  /** Set by the browser in the merged multi-host view. */
  host?: string;
  device: string;
  port: number | null;
  qpn: number;
  qp_type: RdmaQpType;
  state: RdmaQpState;
}
export interface RdmaCmInfo {
  id: number | null;
  /** The CM consumer, which can differ from the QP owner. */
  owner: RdmaEndpointOwner;
  state: RdmaCmState;
  port_space: RdmaPortSpace;
  source: RdmaAddress | null;
  destination: RdmaAddress | null;
}
/** A queue pair; `peer` is set only for a connected QP on the same host. */
export interface RdmaRelation {
  id: string;
  endpoint: RdmaEndpoint;
  peer: RdmaEndpoint | null;
  remote_qpn: number | null;
  cm: RdmaCmInfo | null;
  /** Remote port GID, from an IB CM address or the IPoIB neighbor table. */
  remote_gid: string | null;
  remote_hostname: string | null;
  label: string;
}
export interface RdmaPortInfo {
  /** Set by the browser in the merged multi-host view. */
  host?: string;
  device: string;
  port: number;
  link_layer: string | null;
  state: string | null;
  lid: number | null;
  gid: string | null;
  rate: string | null;
}
export interface SystemSnapshot {
  processes: Process[];
  fd_relations: FdRelation[];
  /** Absent on older servers and recordings. */
  rdma_relations?: RdmaRelation[];
  /** Absent on older servers and recordings. */
  rdma_ports?: RdmaPortInfo[];
  /** Absent on older servers and recordings. */
  hardware_devices?: HardwareDevice[];
  hardware_paths?: HardwarePath[];
  device_handles?: DeviceHandle[];
  sequence?: number;
}
export interface EntryDelta<T> {
  upsert: T[];
  remove: string[];
}
export interface SystemSnapshotUpdate {
  kind: "full" | "delta";
  sequence: number;
  base_sequence?: number;
  processes: (Omit<Process, "maps"> & {
    maps?: MemoryMap[];
    maps_delta?: EntryDelta<MemoryMap>;
  })[];
  fd_relations?: FdRelation[];
  fd_relations_delta?: EntryDelta<FdRelation>;
  rdma_relations?: RdmaRelation[];
  rdma_relations_delta?: EntryDelta<RdmaRelation>;
  rdma_ports?: RdmaPortInfo[];
  hardware_devices?: HardwareDevice[];
  hardware_paths?: HardwarePath[];
  device_handles?: DeviceHandle[];
  device_handles_delta?: EntryDelta<DeviceHandle>;
}
export interface IoActivity {
  process_id: ProcessId;
  resource: IpcIdentity;
  write: boolean;
  bytes: number;
  count: number;
}
export interface FileActivity {
  process_id: ProcessId;
  file: FileIdentity;
  path: string | null;
  write: boolean;
  bytes: number;
  count: number;
}
export interface CpuActivity {
  process_id: ProcessId;
  runtime_ns: number;
  switches: number;
  running_threads: number;
  cpus: number[];
}
/** signal_generate observation; timestamp is monotonic kernel nanoseconds. */
export interface SignalEvent {
  timestamp_ns: number;
  src_pid: number;
  dst_pid: number;
  signal: number;
  source_id: ProcessId;
  destination_id: ProcessId;
}
export interface SpaceActivity {
  status: SystemMonitorStatus;
  window_ms: number;
  files?: FileActivity[];
  cpu?: CpuActivity[];
  ipc?: IoActivity[];
  signals?: SignalEvent[];
}
export type HostStatus =
  { state: "online"; since: number } | { state: "offline"; since: number };
/** `GET /api/hosts` entries; the local host is first. */
export interface HostSummary {
  name: string;
  local: boolean;
  status: HostStatus;
  machine_id: string;
  version: string;
  kernel: string | null;
  /** The procinsh build architecture, such as `x86_64` or `aarch64`. */
  arch: string;
}
export interface HostsResponse {
  local: string;
  hosts: HostSummary[];
}
export interface CrossHostLinkEnd {
  host: string;
  relation: string;
}
/** `links` events of the merged stream: connections observed on two hosts. */
export interface CrossHostLinks {
  sockets: { a: CrossHostLinkEnd; b: CrossHostLinkEnd }[];
  /** Absent on servers without RDMA support. */
  rdma?: {
    a: CrossHostLinkEnd;
    b: CrossHostLinkEnd;
    evidence: { gid: boolean; cm: boolean };
  }[];
}
