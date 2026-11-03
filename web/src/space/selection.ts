// Own selection and hover state, and translate pointer hits into user actions.

import { spaceNow } from "./clock.js";
import * as T from "three";
import { Display } from "../shared/display.js";
import {
  connectionState,
  fileLabel,
  remoteLabel,
  findNearestRegion,
} from "./model.js";
export { findNearestRegion };
import { rdmaEndpointLabel } from "./rdma.js";
import type {
  RecentFile,
  NetlinkSocketStats,
  NetworkGroup,
  ConnectionPick,
  PickResult,
  SelectionState,
  HoverState,
  Region,
} from "./types.js";
import type {
  SelectionTargets,
  SelectionBindings,
  PickingView,
  RdmaInput,
} from "./contracts.js";
export class SpaceSelection implements SelectionState, HoverState {
  process: string | null = null;
  connection: string | null = null;
  network: string | null = null;
  file: string | null = null;
  netlink: string | null = null;
  hardware: string | null = null;
  rdma: string | null = null;
  hoveredNetwork: string | null = null;
  hoveredFile: string | null = null;
  hoveredNetlink: string | null = null;
  hoveredHardware: string | null = null;
  hoveredRdma: string | null = null;
  get active() {
    return !!(
      this.process ||
      this.connection ||
      this.network ||
      this.file ||
      this.netlink ||
      this.hardware ||
      this.rdma
    );
  }
  clear() {
    this.process =
      this.connection =
      this.network =
      this.file =
      this.netlink =
      this.hardware =
      this.rdma =
        null;
  }
  choose(
    kind:
      | "process"
      | "connection"
      | "network"
      | "file"
      | "netlink"
      | "hardware"
      | "rdma",
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
      (this.file && !targets.files.has(this.file)) ||
      (this.netlink && !targets.netlink.has(this.netlink)) ||
      (this.hardware && !targets.hardware?.has(this.hardware)) ||
      (this.rdma && !targets.rdma?.has(this.rdma))
    )
      this.clear();
  }
}
// Three.js userData is an untyped extension point. Only these picking fields are
// written by scene.ts; narrow at the boundary instead of leaking it into UI state.
interface PickData {
  files?: RecentFile[];
  netlink?: NetlinkSocketStats[];
  netlinkPaths?: string[];
  filePaths?: string[];
  network?: NetworkGroup[];
  fd_relations?: (ConnectionPick | null)[];
  /** PCI addresses of hardware devices and PCIe ribbons by instance. */
  hardware?: string[];
  /** The device of each process-to-device line segment. */
  hardwareLinks?: string[];
  /** RDMA group IDs of QP markers and device IDs of plates, by instance. */
  rdma?: string[];
  rdmaDevices?: string[];
  /** The group or link of each RDMA line segment. */
  rdmaPaths?: string[];
}
function rdmaHover(id: string, rdma: RdmaInput | undefined) {
  const group = rdma?.groups.get(id);
  if (group)
    return `${group.label}\n${group.members.map((r) => rdmaEndpointLabel(r.endpoint)).join(" · ")}`;
  const link = rdma?.links.find((l) => l.id === id);
  if (link)
    return `${link.relation.label}\n${rdmaEndpointLabel(link.relation.endpoint)} ↔ ${link.relation.peer ? rdmaEndpointLabel(link.relation.peer) : ""}`;
  const device = rdma?.devices.get(id);
  if (device)
    return `${device.label} · RDMA device\n${device.ports
      .map(
        (p) =>
          `Port ${p.port} ${p.state ?? ""}${p.lid === null ? "" : ` · LID ${p.lid}`}`,
      )
      .join("\n")}`;
  return "";
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
      const socket =
        result.instanceId === undefined
          ? undefined
          : data.netlink?.[result.instanceId];
      if (socket) return { netlinkId: socket.id };
      const netlinkId =
        result.index === undefined
          ? undefined
          : data.netlinkPaths?.[Math.floor(result.index / 2)];
      if (netlinkId) return { netlinkId };
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
      const hardwareId =
        result.instanceId === undefined
          ? result.index === undefined
            ? undefined
            : data.hardwareLinks?.[Math.floor(result.index / 2)]
          : data.hardware?.[result.instanceId];
      if (hardwareId) return { hardwareId };
      const rdmaId =
        result.instanceId === undefined
          ? result.index === undefined
            ? undefined
            : data.rdmaPaths?.[Math.floor(result.index / 2)]
          : (data.rdma?.[result.instanceId] ??
            data.rdmaDevices?.[result.instanceId]);
      if (rdmaId) return { rdmaId };
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
        if ("netlinkId" in edge) actions.netlink(edge.netlinkId);
        else if ("hardwareId" in edge) actions.hardware?.(edge.hardwareId);
        else if ("rdmaId" in edge) actions.rdma?.(edge.rdmaId);
        else if ("fileId" in edge) actions.file(edge.fileId);
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
      hoveredFile: string | null = null,
      hoveredNetlink: string | null = null,
      hoveredHardware: string | null = null,
      hoveredRdma: string | null = null;
    const { view, edgeStats, files, netlink, hardware, rdma } = read();
    const hullIds = view.hullIds,
      nodes = view.nodes;
    const h = hit(e, view);
    let text = "";
    if (h) {
      const id = hullIds[h.instanceId!],
        n = nodes.get(id)!;
      if (n) {
        const r = findNearestRegion(n.regions, h.point.z);
        text = `${n.name} / ${n.identity.pid}${r ? `\n${Display.permissions(r)} ${r.pathname || "anonymous"}\n${r.start} → ${r.end}` : ""}`;
      }
    } else {
      const edge = edgeHit(e, view);
      if (edge && "netlinkId" in edge) {
        hoveredNetlink = edge.netlinkId;
        const socket = netlink.get(edge.netlinkId)!;
        text = `${socket.label}\nPID ${socket.endpoint.process_id.pid} · SEND ${socket.writeBytes} bytes · RECEIVE ${socket.readBytes} bytes`;
      } else if (edge && "hardwareId" in edge) {
        hoveredHardware = edge.hardwareId;
        const device = hardware?.devices.find(
          (d) => d.address === edge.hardwareId,
        );
        if (device)
          text = `${device.label} · ${device.address}\nPCIe ${Display.pcieLink(device.link?.current)}${device.link?.max ? ` · max ${Display.pcieLink(device.link.max)}` : ""}`;
      } else if (edge && "rdmaId" in edge) {
        hoveredRdma = edge.rdmaId;
        text = rdmaHover(edge.rdmaId, rdma);
      } else if (edge && "fileId" in edge) {
        hoveredFile = edge.fileId;
        const f = files.get(edge.fileId)!;
        text = `${f.path || fileLabel(f.file)}\nPID ${f.process_id.pid} · READ ${f.readBytes} bytes · WRITE ${f.writeBytes} bytes`;
      } else if (edge) {
        hoveredNetwork = edge.networkId || null;
        const stat = edgeStats.get(edge.id);
        text = `${edge.label}${edge.candidate ? " · Candidate peer" : ""}${edge.shared ? " · Shared FD" : ""}\nPID ${edge.endpoint.process_id.pid} / FD ${edge.endpoint.fd}${edge.endpoint.fd_count > 1 ? ` (+${edge.endpoint.fd_count - 1} shared FDs)` : ""} ↔ ${edge.peer ? `PID ${edge.peer.process_id.pid} / FD ${edge.peer.fd}` : edge.socket?.network_peer ? remoteLabel(edge.socket) : connectionState(edge)}${stat ? `\nLatest window: ${stat.bytes} bytes / ${stat.count} operations (${((spaceNow() - stat.time) / 1000).toFixed(1)}s ago)` : ""}`;
      }
    }
    changedHover({
      hoveredNetwork,
      hoveredFile,
      hoveredNetlink,
      hoveredHardware,
      hoveredRdma,
    });
    hover.hidden = !text;
    if (text) {
      hover.textContent = text;
      hover.style.left = `${Math.max(0, Math.min(e.clientX + 15, innerWidth - 350))}px`;
      hover.style.top = `${e.clientY + 15}px`;
    }
  };
  const pointerleave = () => {
    hover.hidden = true;
    changedHover({
      hoveredNetwork: null,
      hoveredFile: null,
      hoveredNetlink: null,
      hoveredHardware: null,
      hoveredRdma: null,
    });
  };

  canvas.addEventListener("pointerdown", pointerdown);
  canvas.addEventListener("pointerup", pointerup);
  canvas.addEventListener("dblclick", dblclick);
  canvas.addEventListener("pointermove", pointermove);
  canvas.addEventListener("pointerleave", pointerleave);
  return () => {
    canvas.removeEventListener("pointerdown", pointerdown);
    canvas.removeEventListener("pointerup", pointerup);
    canvas.removeEventListener("dblclick", dblclick);
    canvas.removeEventListener("pointermove", pointermove);
    canvas.removeEventListener("pointerleave", pointerleave);
  };
}
