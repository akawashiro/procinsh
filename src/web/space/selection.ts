// Own selection and hover state, and translate pointer hits into user actions.
import * as T from "/vendor/three.module.js";
import { Display } from "../shared/display.js";
import { connectionState, fileLabel, remoteLabel } from "./model.js";
import type {
  RecentFile,
  NetworkGroup,
  ConnectionPick,
  PickResult,
  SelectionState,
  HoverState,
} from "./types.js";
import type {
  SelectionTargets,
  SelectionBindings,
  PickingView,
} from "./contracts.js";
export class SpaceSelection implements SelectionState, HoverState {
  process: string | null = null;
  connection: string | null = null;
  network: string | null = null;
  file: string | null = null;
  hoveredNetwork: string | null = null;
  hoveredFile: string | null = null;
  get active() {
    return !!(this.process || this.connection || this.network || this.file);
  }
  clear() {
    this.process = this.connection = this.network = this.file = null;
  }
  choose(
    kind: "process" | "connection" | "network" | "file",
    id: string | null,
  ) {
    this.clear();
    this[kind] = id;
  }
  retain(targets: SelectionTargets) {
    if (
      (this.process && !targets.processes.has(this.process)) ||
      (this.connection && !targets.connections.has(this.connection)) ||
      (this.network && !targets.networks.has(this.network)) ||
      (this.file && !targets.files.has(this.file))
    )
      this.clear();
  }
}
// Three.js userData is an untyped extension point. Only these picking fields are
// written by scene.ts; narrow at the boundary instead of leaking it into UI state.
interface PickData {
  files?: RecentFile[];
  filePaths?: string[];
  network?: NetworkGroup[];
  fd_relations?: (ConnectionPick | null)[];
}
function pickData(object: T.Object3D): PickData {
  return object.userData as PickData;
}

export function bindSelection(
  canvas: HTMLCanvasElement,
  hover: HTMLElement,
  { camera, read, actions, hover: changedHover }: SelectionBindings,
) {
  const ray = new T.Raycaster(),
    mouse = new T.Vector2();
  let down: { x: number; y: number } | undefined;
  function setRay(event: MouseEvent) {
    mouse.set(
      (event.clientX / innerWidth) * 2 - 1,
      (-event.clientY / innerHeight) * 2 + 1,
    );
    ray.setFromCamera(mouse, camera);
  }
  function hit(event: MouseEvent, view: PickingView) {
    const hull = view.hull;
    setRay(event);
    return hull && ray.intersectObject(hull)[0];
  }
  function edgeHit(event: MouseEvent, view: PickingView): PickResult | null {
    setRay(event);
    ray.params.Line.threshold = 0.25;
    for (const result of ray.intersectObjects(view.objects)) {
      const data = pickData(result.object);
      const file =
        result.instanceId === undefined
          ? undefined
          : data.files?.[result.instanceId];
      if (file) return { fileId: file.id };
      const fileId =
        result.index === undefined
          ? undefined
          : data.filePaths?.[Math.floor(result.index / 2)];
      if (fileId) return { fileId };
      const group =
        result.instanceId === undefined
          ? undefined
          : data.network?.[result.instanceId];
      if (group)
        return {
          ...group,
          peer: null,
          shared: false,
          candidate: false,
          networkId: group.id,
        };
      const edge =
        result.index === undefined
          ? undefined
          : data.fd_relations?.[Math.floor(result.index / 2)];
      if (edge) return edge;
    }
    return null;
  }
  const pointerdown = (e: PointerEvent) => {
    down = { x: e.clientX, y: e.clientY };
  };
  const pointerup = (e: PointerEvent) => {
    if (!down || Math.hypot(e.clientX - down.x, e.clientY - down.y) > 5) return;
    const { view } = read();
    const hullIds = view.hullIds;
    const h = hit(e, view);
    if (h) actions.process(hullIds[h.instanceId!]);
    else {
      const edge = edgeHit(e, view);
      if (edge) {
        if ("fileId" in edge) actions.file(edge.fileId);
        else if (edge.networkId) actions.network(edge.networkId);
        else actions.connection(edge.id);
      } else actions.clear();
    }
  };
  const dblclick = (e: MouseEvent) => {
    const { view } = read();
    const hullIds = view.hullIds;
    const h = hit(e, view);
    if (h) actions.process(hullIds[h.instanceId!], true);
  };
  const pointermove = (e: PointerEvent) => {
    let hoveredNetwork: string | null = null,
      hoveredFile: string | null = null;
    const { view, edgeStats, files } = read();
    const hullIds = view.hullIds,
      nodes = view.nodes;
    const h = hit(e, view);
    let text = "";
    if (h) {
      const id = hullIds[h.instanceId!],
        n = nodes.get(id)!,
        r = n.regions.find((r) => h.point.z >= r.z && h.point.z <= r.z + r.h);
      text = `${n.name} / ${n.identity.pid}${r ? `\n${Display.permissions(r)} ${r.pathname || "anonymous"}\n${r.start} → ${r.end}` : ""}`;
    } else {
      const edge = edgeHit(e, view);
      if (edge && "fileId" in edge) {
        hoveredFile = edge.fileId;
        const f = files.get(edge.fileId)!;
        text = `${f.path || fileLabel(f.file)}\nPID ${f.process_id.pid} · READ ${f.readBytes} bytes · WRITE ${f.writeBytes} bytes`;
      } else if (edge) {
        hoveredNetwork = edge.networkId || null;
        const stat = edgeStats.get(edge.id);
        text = `${edge.label}${edge.candidate ? " · Candidate peer" : ""}${edge.shared ? " · Shared FD" : ""}\nPID ${edge.endpoint.process_id.pid} / FD ${edge.endpoint.fd}${edge.endpoint.fd_count > 1 ? ` (+${edge.endpoint.fd_count - 1} shared FDs)` : ""} ↔ ${edge.peer ? `PID ${edge.peer.process_id.pid} / FD ${edge.peer.fd}` : edge.socket?.network_peer ? remoteLabel(edge.socket) : connectionState(edge)}${stat ? `\nLatest window: ${stat.bytes} bytes / ${stat.count} operations (${((performance.now() - stat.time) / 1000).toFixed(1)}s ago)` : ""}`;
      }
    }
    changedHover({ hoveredNetwork, hoveredFile });
    hover.hidden = !text;
    if (text) {
      hover.textContent = text;
      hover.style.left = `${Math.max(0, Math.min(e.clientX + 15, innerWidth - 350))}px`;
      hover.style.top = `${e.clientY + 15}px`;
    }
  };

  canvas.addEventListener("pointerdown", pointerdown);
  canvas.addEventListener("pointerup", pointerup);
  canvas.addEventListener("dblclick", dblclick);
  canvas.addEventListener("pointermove", pointermove);
  return () => {
    canvas.removeEventListener("pointerdown", pointerdown);
    canvas.removeEventListener("pointerup", pointerup);
    canvas.removeEventListener("dblclick", dblclick);
    canvas.removeEventListener("pointermove", pointermove);
  };
}
