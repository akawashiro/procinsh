// Pure identity, resource labels, and process colors shared by SPACE components.
import { Display } from "../shared/display.js";
import type {
  ProcessId,
  SocketEndpoint,
  FdRelation,
  IpcIdentity,
  FileIdentity,
} from "../shared/api-types.js";
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
  if (e.socket?.state.kind === "listen") return "Listening";
  if (e.socket?.protocol.kind === "udp") return "No destination set";
  return "Unknown destination";
}

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
