// Pure identity, resource labels, process dimensions and colors for SPACE.
import { Display } from "../shared/display.js";
import type {
  ProcessId,
  FdEndpoint,
  SocketEndpoint,
  FdRelation,
  IpcIdentity,
  FileIdentity,
  MemoryMap,
} from "../shared/api-types.js";
import type { Region } from "./types.js";
export const PROCESS_SIDE = 2.2;
export const remoteLabel = (socket: SocketEndpoint | null | undefined) =>
  socket?.remote_hostname && socket.remote
    ? `${socket.remote_hostname}:${socket.remote.port}`
    : Display.address(socket?.remote);
export const key = (id: ProcessId) => `${id.pid}:${id.start_time_ticks}`;

export function connectionState(
  e: Pick<FdRelation, "shared" | "candidate" | "peer" | "socket">,
) {
  if (e.shared) return "Shared FD";
  if (e.candidate) return "Candidate peer";
  if (e.peer) return "Confirmed process connection";
  if (e.socket?.network_peer) return "Network destination";
  if (e.socket?.state?.kind === "listen") return "Listening";
  if (
    e.socket?.protocol.kind === "udp" &&
    (e.socket.remote || e.socket.state?.kind === "unconnected")
  )
    return "No destination set";
  return "Unknown destination";
}

/** Positive exclusion only: missing/unknown observations remain visible. */
export function excludedSocket(endpoint: FdEndpoint | null) {
  return (
    endpoint?.resource.kind === "socket" &&
    (endpoint.connection_status === "listening" ||
      endpoint.connection_status === "unconnected")
  );
}

export function isSpaceRelationVisible(relation: FdRelation) {
  return !excludedSocket(relation.endpoint) && !excludedSocket(relation.peer);
}

/** Process identity plus the complete resource identity, shared with IPC events. */
export const socketOwnerKey = (
  endpoint: Pick<FdEndpoint, "process_id" | "resource">,
) => JSON.stringify([key(endpoint.process_id), ipcKey(endpoint.resource)]);

export function userColor(uid: number | null | undefined) {
  if (uid === null || uid === undefined) return "#889299";
  let hash = Number(uid) >>> 0;
  hash = Math.imul(hash ^ (hash >>> 16), 0x45d9f3b);
  hash = Math.imul(hash ^ (hash >>> 16), 0x45d9f3b);
  const hue = ((hash ^ (hash >>> 16)) >>> 0) % 360;
  return `hsl(${hue}, 65%, 65%)`;
}
export const processColors = (n: {
  uid?: number | null;
  euid?: number | null;
}) => ({ real: userColor(n.uid), effective: userColor(n.euid) });

export const ipcKey = (r: IpcIdentity) =>
  JSON.stringify([r.kind, r.device.major, r.device.minor, r.inode]);
export const ipcLabel = (r: IpcIdentity) =>
  `${r.kind}:${r.device.major}:${r.device.minor}:${r.inode}`;
export const fileLabel = (r: FileIdentity) =>
  `file:${r.device.major}:${r.device.minor}:${r.inode}:${r.generation}`;

/** Linux x86-64 signal numbers; kernel realtime signals include reserved 32/33. */
export function signalName(signal: number) {
  const names = [
    "",
    "SIGHUP",
    "SIGINT",
    "SIGQUIT",
    "SIGILL",
    "SIGTRAP",
    "SIGABRT",
    "SIGBUS",
    "SIGFPE",
    "SIGKILL",
    "SIGUSR1",
    "SIGSEGV",
    "SIGUSR2",
    "SIGPIPE",
    "SIGALRM",
    "SIGTERM",
    "SIGSTKFLT",
    "SIGCHLD",
    "SIGCONT",
    "SIGSTOP",
    "SIGTSTP",
    "SIGTTIN",
    "SIGTTOU",
    "SIGURG",
    "SIGXCPU",
    "SIGXFSZ",
    "SIGVTALRM",
    "SIGPROF",
    "SIGWINCH",
    "SIGIO",
    "SIGPWR",
    "SIGSYS",
  ];
  return (
    names[signal] ||
    (signal >= 32 && signal <= 64 ? `SIGRT${signal - 32}` : `SIG${signal}`)
  );
}

export function layoutMaps<M extends Pick<MemoryMap, "start" | "end">>(
  maps: M[],
) {
  // Parse each address once; comparing BigInts parsed inside the sort
  // comparator dominated snapshot updates.
  const sorted = maps
    .map((m) => ({ m, start: BigInt(m.start) }))
    .sort((a, b) => (a.start < b.start ? -1 : 1));
  const sizes = sorted.map(({ m, start }) => Number(BigInt(m.end) - start));
  const total = sizes.reduce((a, b) => a + b, 0) || 1,
    gap = 0.018;
  const heights = sizes.map((size) => Math.max(0.008, (size / total) * 7));
  let end = 0;
  for (const h of heights) end += h + gap;
  let z = 0;
  return sorted.map(({ m }, i) => {
    const h = heights[i];
    const result = { ...m, z: (z / end) * 8, h: (h / end) * 8 };
    z += h + gap;
    return result;
  });
}

export function findNearestRegion(
  regions: readonly Region[] | undefined,
  z: number,
): Region | undefined {
  if (!regions || regions.length === 0) return undefined;
  for (let i = 0; i < regions.length; i++) {
    const r = regions[i];
    if (z >= r.z && z <= r.z + r.h) return r;
  }
  let best = regions[0];
  let bestDist = Infinity;
  for (let i = 0; i < regions.length; i++) {
    const r = regions[i];
    const dist = z < r.z ? r.z - z : z > r.z + r.h ? z - (r.z + r.h) : 0;
    if (dist < bestDist) {
      bestDist = dist;
      best = r;
    }
  }
  return best;
}
