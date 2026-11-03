// Build hardware geometry and selection overlays; the caller owns and disposes their groups.

import * as T from "three";
import type { SceneInput } from "./contracts.js";
import { key, PROCESS_SIDE } from "./model.js";
import { Display } from "../shared/display.js";
import type { HardwareDevice } from "../shared/api-types.js";
import { deviceUsers, downgraded, pathLabel } from "./hardware.js";
import type {
  RenderNode,
  HardwareView,
  HardwarePathView,
  SelectionState,
} from "./types.js";

const HARDWARE_STYLE = {
  gpu: { color: 0xf0b44c, size: [2.2, 2.2, 3] },
  rdma: { color: 0xb48cff, size: [2.2, 2.2, 0.5] },
  accelerator: { color: 0x4fd1c5, size: [2.2, 2.2, 2] },
  nv_switch: { color: 0x9aa7b0, size: [2.2, 1.2, 1] },
  host_bridge: { color: 0x8aa0aa, size: [3, 1.4, 0.6] },
  port: { color: 0x6d8a96, size: [1, 1, 0.3] },
} as const;
/** PCIe ribbon colors by generation; slower links are darker. */
const PCIE_COLORS = [
  0x5c6f7a, 0x5c6f7a, 0x3fa7a0, 0x46c8e0, 0x7fd8ff, 0xb8f0ff,
];
const DOWNGRADED_COLOR = 0xe0a040;
const PATH_COLORS = {
  pix: 0x6cda92,
  pxb: 0xb5d96c,
  phb: 0xe0c060,
  node: 0xf09050,
  sys: 0xf06060,
} as const;
function hardwareStyle(device: HardwareDevice) {
  if (device.kind) return HARDWARE_STYLE[device.kind];
  return device.role === "host_bridge"
    ? HARDWARE_STYLE.host_bridge
    : HARDWARE_STYLE.port;
}
/** Devices, bridges and PCIe ribbons of the hardware bay in front of the processes. */
export function buildHardware(
  input: SceneInput,
  nodes: ReadonlyMap<string, RenderNode>,
  geometryGroup: T.Group,
) {
  const dummy = new T.Object3D();
  const hardwareViews: HardwareView[] = [];
  const hardware = input.hardware;
  if (!hardware?.devices.length) return hardwareViews;
  const placed = hardware.devices.filter((d) =>
    hardware.positions.has(d.address),
  );
  const byAddress = new Map(placed.map((d) => [d.address, d]));
  const users = deviceUsers(hardware.handles, (h) => key(h.process_id));
  const boxes = new T.InstancedMesh(
    new T.BoxGeometry(1, 1, 1),
    new T.MeshBasicMaterial({
      transparent: true,
      opacity: 0.3,
      depthWrite: false,
    }),
    placed.length,
  );
  boxes.name = "hardware-devices";
  boxes.userData.hardware = placed.map((d) => d.address);
  const edges: number[] = [],
    edgeColors: number[] = [];
  const corners = new T.BoxGeometry(1, 1, 1);
  const outline = new T.EdgesGeometry(corners);
  corners.dispose();
  const outlinePoints = outline.getAttribute("position");
  placed.forEach((device, i) => {
    const p = hardware.positions.get(device.address)!,
      style = hardwareStyle(device),
      [sx, sy, sz] = style.size;
    dummy.position.set(p.x, p.y, p.z + sz / 2);
    dummy.scale.set(sx, sy, sz);
    dummy.updateMatrix();
    boxes.setMatrixAt(i, dummy.matrix);
    const color = new T.Color(style.color);
    boxes.setColorAt(i, color);
    const point = new T.Vector3();
    for (let j = 0; j < outlinePoints.count; j++) {
      point.fromBufferAttribute(outlinePoints, j).applyMatrix4(dummy.matrix);
      edges.push(point.x, point.y, point.z);
      edgeColors.push(color.r, color.g, color.b);
    }
    hardwareViews.push({
      device,
      pos: new T.Vector3(p.x, p.y, p.z + sz),
      users: users.get(device.address)?.size ?? 0,
    });
  });
  outline.dispose();
  dummy.scale.set(1, 1, 1);
  geometryGroup.add(boxes);
  const edgeGeometry = new T.BufferGeometry();
  edgeGeometry.setAttribute("position", new T.Float32BufferAttribute(edges, 3));
  edgeGeometry.setAttribute(
    "color",
    new T.Float32BufferAttribute(edgeColors, 3),
  );
  geometryGroup.add(
    new T.LineSegments(
      edgeGeometry,
      new T.LineBasicMaterial({
        vertexColors: true,
        transparent: true,
        opacity: 0.85,
      }),
    ),
  );
  // Flat orthogonal ribbons from each node to its upstream port, like parent lines.
  const ribbons: {
    start: T.Vector3;
    end: T.Vector3;
    width: number;
    color: number;
    child: string;
  }[] = [];
  for (const device of placed) {
    const parent = device.parent && byAddress.get(device.parent);
    if (!parent) continue;
    const a = hardware.positions.get(device.address)!,
      b = hardware.positions.get(parent.address)!;
    const link = device.link?.current ?? null;
    const color = downgraded(device, parent)
      ? DOWNGRADED_COLOR
      : link
        ? PCIE_COLORS[Math.min(5, Display.pcieGeneration(link.speed_gts) - 1)]
        : PCIE_COLORS[0];
    const width = 0.12 + (link?.width ?? 1) * 0.02;
    // Levels run along x: turn halfway between a node and its parent.
    const middle = (a.x + b.x) / 2;
    const points = [
      new T.Vector3(a.x, a.y, a.z + 0.05),
      new T.Vector3(middle, a.y, a.z + 0.05),
      new T.Vector3(middle, b.y, b.z + 0.05),
      new T.Vector3(b.x, b.y, b.z + 0.05),
    ];
    for (let i = 1; i < points.length; i++)
      if (!points[i - 1].equals(points[i]))
        ribbons.push({
          start: points[i - 1],
          end: points[i],
          width,
          color,
          child: device.address,
        });
  }
  const ribbonMesh = new T.InstancedMesh(
    new T.PlaneGeometry(1, 1),
    new T.MeshBasicMaterial({ side: T.DoubleSide, depthWrite: false }),
    Math.max(1, ribbons.length),
  );
  ribbonMesh.count = ribbons.length;
  ribbonMesh.name = "pcie-links";
  ribbonMesh.userData.hardware = ribbons.map((r) => r.child);
  ribbons.forEach((ribbon, i) => {
    dummy.position.copy(ribbon.start).lerp(ribbon.end, 0.5);
    dummy.rotation.z = Math.atan2(
      ribbon.end.y - ribbon.start.y,
      ribbon.end.x - ribbon.start.x,
    );
    dummy.scale.set(
      ribbon.start.distanceTo(ribbon.end) + ribbon.width,
      ribbon.width,
      1,
    );
    dummy.updateMatrix();
    ribbonMesh.setMatrixAt(i, dummy.matrix);
    ribbonMesh.setColorAt(i, new T.Color(ribbon.color));
  });
  dummy.rotation.set(0, 0, 0);
  dummy.scale.set(1, 1, 1);
  geometryGroup.add(ribbonMesh);
  // Every process holding a device is joined to it; selection brightens these links.
  const linkPositions: number[] = [],
    linkColors: number[] = [],
    linkTargets: string[] = [];
  for (const { address, curve, color } of userLinks(
    nodes,
    hardware,
    input.visible,
  )) {
    const c = new T.Color(color).multiplyScalar(0.55);
    const points = curve.getPoints(16);
    for (let i = 1; i < points.length; i++) {
      linkPositions.push(...points[i - 1].toArray(), ...points[i].toArray());
      linkColors.push(c.r, c.g, c.b, c.r, c.g, c.b);
      linkTargets.push(address);
    }
  }
  const userGeometry = new T.BufferGeometry();
  userGeometry.setAttribute(
    "position",
    new T.Float32BufferAttribute(linkPositions, 3),
  );
  userGeometry.setAttribute(
    "color",
    new T.Float32BufferAttribute(linkColors, 3),
  );
  const userLines = new T.LineSegments(
    userGeometry,
    new T.LineBasicMaterial({
      vertexColors: true,
      transparent: true,
      opacity: 0.6,
      depthWrite: false,
      blending: T.AdditiveBlending,
    }),
  );
  userLines.name = "hardware-users";
  userLines.userData.hardwareLinks = linkTargets;
  geometryGroup.add(userLines);
  return hardwareViews;
}

/**
 * One curve per process and device it holds, from the process's front face to the
 * device roof. `visible` limits the processes; all are used when it is omitted.
 */
function userLinks(
  nodes: ReadonlyMap<string, RenderNode>,
  hardware: NonNullable<SceneInput["hardware"]>,
  visible?: ReadonlySet<string>,
) {
  const devices = new Map(hardware.devices.map((d) => [d.address, d]));
  const links: {
    process: string;
    address: string;
    curve: T.CubicBezierCurve3;
    color: number;
  }[] = [];
  for (const [address, byProcess] of deviceUsers(hardware.handles, (h) =>
    key(h.process_id),
  )) {
    const device = devices.get(address),
      p = hardware.positions.get(address);
    if (!device || !p) continue;
    const roof = new T.Vector3(p.x, p.y, p.z + hardwareStyle(device).size[2]);
    const color =
      device.kind === "rdma"
        ? HARDWARE_STYLE.rdma.color
        : HARDWARE_STYLE.gpu.color;
    for (const process of byProcess.keys()) {
      const n = nodes.get(process);
      if (!n || (visible && !visible.has(process))) continue;
      const start = new T.Vector3(
        n.pos.x,
        n.pos.y - PROCESS_SIDE / 2,
        n.pos.z + 1,
      );
      links.push({
        process,
        address,
        color,
        curve: new T.CubicBezierCurve3(
          start,
          start.clone().add(new T.Vector3(0, -3, 0)),
          roof.clone().add(new T.Vector3(0, 3, 2)),
          roof,
        ),
      });
    }
  }
  return links;
}

/**
 * Selection-dependent hardware lines: PCIe paths from a selected device, and
 * descriptor links between processes and devices for a selected process or device.
 */
export function buildHardwareOverlay(
  hardware: SceneInput["hardware"],
  nodes: ReadonlyMap<string, RenderNode>,
  hardwareOverlay: T.Group,
  selectionState: SelectionState,
) {
  const hardwarePathViews: HardwarePathView[] = [];
  if (!hardware) return hardwarePathViews;
  const selected = selectionState.hardware ?? null;
  const at = (address: string, lift: number) => {
    const p = hardware.positions.get(address);
    return p && new T.Vector3(p.x, p.y, p.z + lift);
  };
  const positions: number[] = [],
    colors: number[] = [];
  const line = (points: T.Vector3[], color: number) => {
    const c = new T.Color(color);
    for (let i = 1; i < points.length; i++) {
      positions.push(...points[i - 1].toArray(), ...points[i].toArray());
      colors.push(c.r, c.g, c.b, c.r, c.g, c.b);
    }
  };
  if (selected)
    for (const path of hardware.paths) {
      if (path.a !== selected && path.b !== selected) continue;
      const ordered =
        path.a === selected
          ? [path.a, ...path.via, path.b]
          : [path.b, ...[...path.via].reverse(), path.a];
      const points = ordered
        .map((address) => at(address, 0.6))
        .filter((p): p is T.Vector3 => !!p);
      if (points.length < 2) continue;
      line(points, PATH_COLORS[path.class]);
      hardwarePathViews.push({
        path,
        label: pathLabel(path),
        pos: points[points.length - 1].clone().add(new T.Vector3(0, -1.2, 0)),
      });
    }
  for (const { process, address, curve, color } of userLinks(nodes, hardware))
    if (selected === address || selectionState.process === process)
      line(curve.getPoints(16), color);
  if (!positions.length) return hardwarePathViews;
  const geometry = new T.BufferGeometry();
  geometry.setAttribute("position", new T.Float32BufferAttribute(positions, 3));
  geometry.setAttribute("color", new T.Float32BufferAttribute(colors, 3));
  hardwareOverlay.add(
    new T.LineSegments(
      geometry,
      new T.LineBasicMaterial({
        vertexColors: true,
        transparent: true,
        opacity: 0.95,
        depthWrite: false,
        blending: T.AdditiveBlending,
      }),
    ),
  );
  return hardwarePathViews;
}
