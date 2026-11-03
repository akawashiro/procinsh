// Inputs and callbacks for SPACE geometry, picking, details, and rendering.

import type * as T from "three";
import type {
  DeviceHandle,
  HardwareDevice,
  HardwarePath,
  SystemSnapshot,
} from "../shared/api-types.js";
import type {
  PlacedProcess,
  Position,
  NetworkGroup,
  RecentFile,
  NetlinkSocketStats,
  NetlinkView,
  EdgeStat,
  RenderNode,
  EdgeView,
  NetworkView,
  FileView,
  HoverState,
  HardwarePathView,
  HardwareView,
  HostLayerView,
  RdmaDevice,
  RdmaDeviceView,
  RdmaGroup,
  RdmaLink,
  RdmaLinkView,
  RdmaView,
} from "./types.js";

export interface SelectionActions {
  process(id: string | null, focus?: boolean): void;
  connection(id: string | null): void;
  network(id: string | null): void;
  file(id: string | null): void;
  netlink(id: string | null): void;
  hardware?(id: string | null): void;
  rdma?(id: string | null): void;
  clear(): void;
}
export interface SelectionTargets {
  processes: ReadonlySet<string>;
  connections: ReadonlySet<string>;
  networks: ReadonlySet<string>;
  files: ReadonlySet<string>;
  netlink: ReadonlySet<string>;
  hardware?: ReadonlySet<string>;
  rdma?: ReadonlySet<string>;
}
/** RDMA groups, QP links and device anchors. */
export interface RdmaInput {
  groups: ReadonlyMap<string, RdmaGroup>;
  links: readonly RdmaLink[];
  devices: ReadonlyMap<string, RdmaDevice>;
}
/** Devices, paths and descriptor holders of the hardware bay. */
export interface HardwareInput {
  devices: readonly HardwareDevice[];
  paths: readonly HardwarePath[];
  handles: readonly DeviceHandle[];
}
export interface FileSceneInput {
  nodes: ReadonlyMap<string, PlacedProcess>;
  files: ReadonlyMap<string, RecentFile>;
  netlink: ReadonlyMap<string, NetlinkSocketStats>;
  filePositions: ReadonlyMap<string, Position>;
  netlinkPositions: ReadonlyMap<string, Position>;
  visible: ReadonlySet<string>;
}
export interface SceneInput extends FileSceneInput {
  snapshot: SystemSnapshot;
  network: ReadonlyMap<string, NetworkGroup>;
  networkPositions: ReadonlyMap<string, Position>;
  hardware?: HardwareInput & { positions: ReadonlyMap<string, Position> };
  rdma?: RdmaInput & {
    /** Marker positions by group ID. */
    positions: ReadonlyMap<string, Position>;
    /** Device plate positions by device ID. */
    devicePositions: ReadonlyMap<string, Position>;
  };
  /** Floor labels of the merged multi-host view, by host. */
  hostLabels?: ReadonlyMap<string, string>;
}
export interface WorldPositions {
  processes: Iterable<Position>;
  networks: Iterable<Position>;
  files: Iterable<Position>;
  netlink: Iterable<Position>;
}
export interface DetailsInput {
  nodes: ReadonlyMap<string, PlacedProcess>;
  /** The host of process pages for identities without `host`; null for this machine. */
  host?: string | null;
  hardware?: HardwareInput;
  rdma?: RdmaInput;
  snapshot: SystemSnapshot;
  network: ReadonlyMap<string, NetworkGroup>;
  edgeStats: ReadonlyMap<string, EdgeStat>;
  files: ReadonlyMap<string, RecentFile>;
  netlink: ReadonlyMap<string, NetlinkSocketStats>;
}
/** Read afresh for each pointer event, since scene geometry is replaced on updates. */
export interface PickingView {
  hull: T.InstancedMesh | null;
  hullIds: readonly string[];
  nodes: ReadonlyMap<string, RenderNode>;
  objects: T.Object3D[];
}
export interface PickingInput {
  view: PickingView;
  hardware?: HardwareInput;
  rdma?: RdmaInput;
  edgeStats: ReadonlyMap<string, EdgeStat>;
  files: ReadonlyMap<string, RecentFile>;
  netlink: ReadonlyMap<string, NetlinkSocketStats>;
}
export interface SelectionBindings {
  camera: T.Camera;
  read(): PickingInput;
  actions: SelectionActions;
  hover(state: HoverState): void;
}
/** Only geometry and label values consumed by the renderer; no scene operations. */
export interface RenderView {
  nodes: ReadonlyMap<string, RenderNode>;
  hullIds: readonly string[];
  networkViews: readonly NetworkView[];
  fileViews: ReadonlyMap<string, FileView>;
  netlinkViews: ReadonlyMap<string, NetlinkView>;
  hardwareViews?: readonly HardwareView[];
  hardwarePathViews?: readonly HardwarePathView[];
  rdmaViews?: readonly RdmaView[];
  rdmaLinkViews?: readonly RdmaLinkView[];
  rdmaDeviceViews?: readonly RdmaDeviceView[];
  /** Host floors of the merged view; absent or empty for a single host. */
  layerViews?: readonly HostLayerView[];
  edgeViews: readonly EdgeView[];
  baseGlow: T.InstancedMesh | null;
  haloGlow: T.InstancedMesh | null;
}
