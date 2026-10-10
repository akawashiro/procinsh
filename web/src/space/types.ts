// Shared values passed between SPACE components; independent of their implementations.
import type * as T from "three";
import type {
  Process,
  ProcessId,
  FdEndpoint,
  SocketEndpoint,
  FdRelation,
  FileIdentity,
  CpuActivity,
  MemoryMap,
  SignalEvent,
} from "../shared/api-types.js";
export interface Position {
  x: number;
  y: number;
  z: number;
}
export interface TreePosition {
  x: number;
  y: number;
  depth: number;
  parent: string | null;
}
export interface NetworkGroup {
  id: string;
  endpoint: FdEndpoint;
  socket: SocketEndpoint;
  members: FdRelation[];
  label: string;
}
export interface RecentFile {
  id: string;
  process_id: ProcessId;
  file: FileIdentity;
  path: string | null;
  readBytes: number;
  writeBytes: number;
  readCount: number;
  writeCount: number;
  label: string;
  last: number;
}
export type CpuGlow = CpuActivity & { last: number; window_ms: number };
export type Region = MemoryMap & { z: number; h: number };
/** A process with layout coordinates, independent of Three.js objects. */
export interface PlacedProcess extends Process {
  pos: Position;
  regions: Region[];
}
export interface EdgeStat {
  bytes: number;
  count: number;
  time: number;
}
export type ActivityRoute = {
  kind: "file" | "connection" | "port";
  id: string;
  direction?: number | null;
  count: number;
};
export interface ActivityUpdate {
  now: number;
  filesChanged: boolean;
  routes: ActivityRoute[];
  signals?: SignalEvent[];
}
export interface DataEvents {
  snapshot(): void;
  activity(update: ActivityUpdate): void;
  reset(): void;
  gap(): void;
  status(message: string | null): void;
}

export interface SelectionState {
  readonly process: string | null;
  readonly connection: string | null;
  readonly network: string | null;
  readonly file: string | null;
}
export interface HoverState {
  readonly hoveredNetwork: string | null;
  readonly hoveredFile: string | null;
}
export type ConnectionPick = FdRelation & { networkId?: string };
export type PickResult = ConnectionPick | { fileId: string };
export interface RenderNode extends Process {
  pos: T.Vector3;
  regions: Region[];
}
export interface EdgeView {
  e: FdRelation;
  curve: T.QuadraticBezierCurve3;
  networkId?: string;
}
export interface NetworkView {
  group: NetworkGroup;
  pos: T.Vector3;
  curve: T.QuadraticBezierCurve3;
}
export interface FileView {
  file: RecentFile;
  pos: T.Vector3;
  curve: T.QuadraticBezierCurve3;
}
