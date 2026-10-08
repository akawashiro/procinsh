// Inputs and callbacks for SPACE geometry, picking, details, and rendering.
import type * as T from "three";
import type { SystemSnapshot } from "../shared/api-types.js";
import type {
  PlacedProcess,
  Position,
  NetworkGroup,
  RecentFile,
  EdgeStat,
  RenderNode,
  EdgeView,
  NetworkView,
  FileView,
  HoverState,
} from "./types.js";

export interface SelectionActions {
  process(id: string | null, focus?: boolean): void;
  connection(id: string | null): void;
  network(id: string | null): void;
  file(id: string | null): void;
  clear(): void;
}
export interface SelectionTargets {
  processes: ReadonlySet<string>;
  connections: ReadonlySet<string>;
  networks: ReadonlySet<string>;
  files: ReadonlySet<string>;
}
export interface FileSceneInput {
  nodes: ReadonlyMap<string, PlacedProcess>;
  files: ReadonlyMap<string, RecentFile>;
  filePositions: ReadonlyMap<string, Position>;
  visible: ReadonlySet<string>;
}
export interface SceneInput extends FileSceneInput {
  snapshot: SystemSnapshot;
  network: ReadonlyMap<string, NetworkGroup>;
  networkPositions: ReadonlyMap<string, Position>;
}
export interface WorldPositions {
  processes: Iterable<Position>;
  networks: Iterable<Position>;
  files: Iterable<Position>;
}
export interface DetailsInput {
  nodes: ReadonlyMap<string, PlacedProcess>;
  snapshot: SystemSnapshot;
  network: ReadonlyMap<string, NetworkGroup>;
  edgeStats: ReadonlyMap<string, EdgeStat>;
  files: ReadonlyMap<string, RecentFile>;
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
  edgeStats: ReadonlyMap<string, EdgeStat>;
  files: ReadonlyMap<string, RecentFile>;
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
  edgeViews: readonly EdgeView[];
  baseGlow: T.InstancedMesh | null;
  haloGlow: T.InstancedMesh | null;
}
