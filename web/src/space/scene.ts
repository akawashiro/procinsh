// Build and update Three.js geometry from data and selection state.
import * as T from "three";
import { isKernelNetlink } from "./netlink.js";
import {
  key,
  processColors,
  PROCESS_SIDE,
  socketOwnerKey as ipcOwnerKey,
} from "./model.js";
import {
  IpcPortAllocator,
  IPC_STEM_LENGTH,
  IPC_PORT_RADIUS,
} from "./ipc-ports.js";
import type { IpcPort } from "./ipc-ports.js";
import type {
  Region,
  RenderNode,
  EdgeView,
  ConnectionCurve,
  NetworkView,
  FileView,
  NetlinkView,
  ConnectionPick,
  SelectionState,
  HardwarePathView,
  HardwareView,
} from "./types.js";
import { Display } from "../shared/display.js";
import type { HardwareDevice } from "../shared/api-types.js";
import { deviceUsers, downgraded, pathLabel } from "./hardware.js";
import type {
  SceneInput,
  FileSceneInput,
  RenderView,
  PickingView,
} from "./contracts.js";

const PARENT_LINE_WIDTH = 0.6;
const PARENT_LINE_Z = 0.06;
const PARENT_LINE_BRIGHTNESS = 0.22;
const PROCESS_HEIGHT = 8;
const IPC_PORT_COLOR = 0x43c4d4;
const SHARED_FD_COLOR = 0x54828b;
const CANDIDATE_PEER_COLOR = 0xc69a50;
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
const BOX_CORNERS = [
  [-1.1, -1.1],
  [1.1, -1.1],
  [1.1, 1.1],
  [-1.1, 1.1],
];

// Rebuilds color hundreds of thousands of lines and layers from a few dozen
// distinct values; parsing and color-space conversion dominated the rebuild.
// Callers copy from these shared instances and must not mutate them.
const colorCache = new Map<T.ColorRepresentation, T.Color>();
function cachedColor(color: T.ColorRepresentation): T.Color {
  let cached = colorCache.get(color);
  if (!cached) {
    cached = new T.Color(color);
    colorCache.set(color, cached);
  }
  return cached;
}

function ipcCurve(
  start: T.Vector3,
  end: T.Vector3,
  normal: T.Vector3,
  endNormal: T.Vector3,
  self: boolean,
): ConnectionCurve {
  const reach = Math.max(0.6, Math.min(3, start.distanceTo(end) * 0.3));
  if (self && Math.abs(normal.x) === 1 && endNormal.x === -normal.x) {
    // Opposing ports join around the hull instead of passing through its center.
    const middle = start.clone().lerp(end, 0.5);
    middle.y += PROCESS_SIDE / 2 + 1.5;
    const path = new T.CurvePath<T.Vector3>();
    path.add(
      new T.CubicBezierCurve3(
        start,
        start.clone().addScaledVector(normal, reach),
        middle.clone().addScaledVector(normal, reach),
        middle,
      ),
    );
    path.add(
      new T.CubicBezierCurve3(
        middle,
        middle.clone().addScaledVector(normal, -reach),
        end.clone().addScaledVector(normal, -reach),
        end,
      ),
    );
    return path;
  }
  const direction = end.clone().sub(start);
  if (normal.dot(direction) > 0 && endNormal.dot(direction) < 0)
    return new T.CubicBezierCurve3(
      start,
      start.clone().addScaledVector(normal, reach),
      end.clone().addScaledVector(endNormal, reach),
      end,
    );
  // An IPC resource shared by peers in different directions still has one face port.
  // Route behind that face above the hulls rather than cutting through a process.
  const highStart = start
    .clone()
    .addScaledVector(normal, reach)
    .setZ(PROCESS_HEIGHT + 2.5);
  const highEnd = end
    .clone()
    .addScaledVector(endNormal, reach)
    .setZ(PROCESS_HEIGHT + 2.5);
  if (highStart.equals(highEnd)) highEnd.y += 1;
  const up = new T.Vector3(0, 0, reach);
  const path = new T.CurvePath<T.Vector3>();
  path.add(
    new T.CubicBezierCurve3(
      start,
      start.clone().addScaledVector(normal, reach),
      highStart.clone().sub(up),
      highStart,
    ),
  );
  path.add(
    new T.CubicBezierCurve3(
      highStart,
      highStart.clone().add(up),
      highEnd.clone().add(up),
      highEnd,
    ),
  );
  path.add(
    new T.CubicBezierCurve3(
      highEnd,
      highEnd.clone().sub(up),
      end.clone().addScaledVector(endNormal, reach),
      end,
    ),
  );
  return path;
}

type ParentPath = {
  parent: string;
  child: string;
  points: T.Vector3[];
  active: boolean;
};
type ParentSegment = {
  start: T.Vector3;
  end: T.Vector3;
  paths: number[];
};
type ParentJoint = {
  position: T.Vector3;
  paths: number[];
};

// Split collinear overlaps so each floor section has one mesh and all its owners.
function parentSegments(paths: ParentPath[]): ParentSegment[] {
  const rows = new Map<
    string,
    {
      horizontal: boolean;
      fixed: number;
      events: Map<number, { path: number; add: boolean }[]>;
    }
  >();
  paths.forEach((path, index) => {
    for (let i = 1; i < path.points.length; i++) {
      const a = path.points[i - 1],
        b = path.points[i];
      if (a.equals(b)) continue;
      const horizontal = a.y === b.y;
      const fixed = horizontal ? a.y : a.x;
      const id = `${horizontal ? "x" : "y"}:${fixed}`;
      let row = rows.get(id);
      if (!row) {
        row = { horizontal, fixed, events: new Map() };
        rows.set(id, row);
      }
      const start = Math.min(horizontal ? a.x : a.y, horizontal ? b.x : b.y);
      const end = Math.max(horizontal ? a.x : a.y, horizontal ? b.x : b.y);
      for (const [position, add] of [
        [start, true],
        [end, false],
      ] as const) {
        const events = row.events.get(position) ?? [];
        events.push({ path: index, add });
        row.events.set(position, events);
      }
    }
  });
  const result: ParentSegment[] = [];
  for (const row of rows.values()) {
    const coordinates = [...row.events.keys()].sort((a, b) => a - b);
    const active = new Map<number, number>();
    for (let i = 0; i < coordinates.length - 1; i++) {
      const start = coordinates[i],
        end = coordinates[i + 1];
      for (const event of row.events.get(start)!) {
        const count = (active.get(event.path) ?? 0) + (event.add ? 1 : -1);
        if (count) active.set(event.path, count);
        else active.delete(event.path);
      }
      if (!active.size) continue;
      const point = (value: number) =>
        new T.Vector3(
          row.horizontal ? value : row.fixed,
          row.horizontal ? row.fixed : value,
          PARENT_LINE_Z,
        );
      result.push({
        start: point(start),
        end: point(end),
        paths: [...active.keys()],
      });
    }
  }
  return result;
}

// Round caps and joins share the highlight of every path meeting at that point.
function parentJoints(sections: ParentSegment[]): ParentJoint[] {
  const joints = new Map<string, { position: T.Vector3; paths: Set<number> }>();
  for (const section of sections)
    for (const position of [section.start, section.end]) {
      const id = `${position.x}:${position.y}`;
      let joint = joints.get(id);
      if (!joint) {
        joint = { position, paths: new Set() };
        joints.set(id, joint);
      }
      for (const path of section.paths) joint.paths.add(path);
    }
  return [...joints.values()].map(({ position, paths }) => ({
    position,
    paths: [...paths],
  }));
}

/**
 * Free an object's GPU buffers. Materials go to `retired` when given, so a
 * rebuild can release them only after its replacements have rendered.
 */
function disposeObject(object: T.Object3D, retired?: Set<T.Material>) {
  if (object instanceof T.InstancedMesh) object.dispose();
  if (
    object instanceof T.Mesh ||
    object instanceof T.Line ||
    object instanceof T.Points
  ) {
    object.geometry.dispose();
    const materials = Array.isArray(object.material)
      ? object.material
      : [object.material];
    for (const material of materials)
      if (retired) retired.add(material);
      else material.dispose();
  }
}

export function createSpaceScene() {
  const scene = new T.Scene();
  scene.fog = new T.FogExp2(0x03090e, 0.0015);
  // three.js destroys a shader program when its last material is disposed, so
  // disposing the old materials before a rebuild renders forced every snapshot
  // to recompile shaders synchronously (~70 ms). Releasing them after the next
  // render lets the replacements reuse the cached programs.
  const retiredMaterials = new Set<T.Material>();
  function releaseRetiredMaterials() {
    for (const material of retiredMaterials) material.dispose();
    retiredMaterials.clear();
  }
  scene.onAfterRender = releaseRetiredMaterials;
  let nodes = new Map<string, RenderNode>();
  let edgeViews: EdgeView[] = [],
    networkViews: NetworkView[] = [],
    fileViews = new Map<string, FileView>(),
    netlinkViews = new Map<string, NetlinkView>(),
    hardwareViews: HardwareView[] = [],
    hardwarePathViews: HardwarePathView[] = [];
  let hardwareInput: SceneInput["hardware"];
  let hardwareOverlay = new T.Group();
  let parentViews: ParentPath[] = [],
    parentSections: ParentSegment[] = [],
    parentCorners: ParentJoint[] = [],
    parentLines: T.InstancedMesh<T.PlaneGeometry, T.MeshBasicMaterial> | null =
      null,
    parentLineJoints: T.InstancedMesh<
      T.CircleGeometry,
      T.MeshBasicMaterial
    > | null = null;
  let geometryGroup = new T.Group(),
    fileGroup = new T.Group();
  scene.add(geometryGroup, fileGroup);
  let hull: T.InstancedMesh | null = null,
    baseGlow: T.InstancedMesh | null = null,
    haloGlow: T.InstancedMesh | null = null,
    hullIds: string[] = [];
  let ipcPorts: IpcPort[] = [],
    portStems: T.InstancedMesh<T.CylinderGeometry, T.MeshBasicMaterial> | null =
      null;
  const portAllocator = new IpcPortAllocator();
  const portGroup = new T.Group();
  portGroup.name = "ipc-ports";
  const dummy = new T.Object3D();
  function syncNodes(input: FileSceneInput) {
    nodes = new Map(
      [...input.nodes].map(([id, n]) => [
        id,
        { ...n, pos: new T.Vector3(n.pos.x, n.pos.y, n.pos.z) },
      ]),
    );
  }
  const selection = new T.LineSegments(
    new T.EdgesGeometry(new T.BoxGeometry(2.45, 2.45, 8.3)),
    new T.LineBasicMaterial({
      color: 0xd1ffce,
      transparent: true,
      opacity: 0.85,
    }),
  );
  selection.visible = false;
  scene.add(selection);
  const edgeSelection = new T.LineSegments(
    new T.BufferGeometry(),
    new T.LineBasicMaterial({
      color: 0xc9fff2,
      transparent: true,
      opacity: 1,
      depthWrite: false,
      blending: T.AdditiveBlending,
    }),
  );
  edgeSelection.name = "selected-connection";
  edgeSelection.visible = false;
  scene.add(edgeSelection);
  function regionColor(r: Region) {
    const p = r.pathname || "";
    return p.includes("[stack")
      ? 0xab8add
      : p === "[heap]"
        ? 0x6cda92
        : r.executable
          ? 0x5ddad4
          : r.writable
            ? 0x347d91
            : 0x244b5e;
  }
  function disposeGroup() {
    geometryGroup.traverse((object) => disposeObject(object, retiredMaterials));
    scene.remove(geometryGroup);
    geometryGroup = new T.Group();
    scene.add(geometryGroup);
  }
  function buildScene(input: SceneInput, selectedProcess: string | null) {
    syncNodes(input);
    const { snapshot, network, networkPositions } = input;
    disposeGroup();
    const visible = input.visible;
    hullIds = [...visible];
    hull = new T.InstancedMesh(
      new T.BoxGeometry(PROCESS_SIDE, PROCESS_SIDE, PROCESS_HEIGHT),
      new T.MeshBasicMaterial({
        color: 0x4bd6bf,
        transparent: true,
        opacity: 0.025,
        depthWrite: false,
      }),
      Math.max(1, hullIds.length),
    );
    hull.count = hullIds.length;
    geometryGroup.add(hull);
    baseGlow = new T.InstancedMesh(
      new T.BoxGeometry(2.28, 2.28, 0.07),
      new T.MeshBasicMaterial({
        color: 0xffffff,
        transparent: true,
        opacity: 0.95,
        blending: T.AdditiveBlending,
        depthWrite: false,
      }),
      Math.max(1, hullIds.length),
    );
    baseGlow.count = hullIds.length;
    baseGlow.frustumCulled = false;
    geometryGroup.add(baseGlow);
    haloGlow = new T.InstancedMesh(
      new T.BoxGeometry(3.15, 3.15, 0.025),
      new T.MeshBasicMaterial({
        color: 0xffffff,
        transparent: true,
        opacity: 0.32,
        blending: T.AdditiveBlending,
        depthWrite: false,
      }),
      Math.max(1, hullIds.length),
    );
    haloGlow.count = hullIds.length;
    haloGlow.frustumCulled = false;
    geometryGroup.add(haloGlow);
    parentViews = [];
    for (const childId of hullIds) {
      const child = nodes.get(childId)!,
        parentId = child.parent_id && key(child.parent_id),
        parent = parentId && nodes.get(parentId);
      if (!parent || parentId === childId || !visible.has(parentId)) continue;
      const middleY = (parent.pos.y + child.pos.y) / 2;
      parentViews.push({
        parent: parentId,
        child: childId,
        active: false,
        points: [
          new T.Vector3(parent.pos.x, parent.pos.y, PARENT_LINE_Z),
          new T.Vector3(parent.pos.x, middleY, PARENT_LINE_Z),
          new T.Vector3(child.pos.x, middleY, PARENT_LINE_Z),
          new T.Vector3(child.pos.x, child.pos.y, PARENT_LINE_Z),
        ],
      });
    }
    parentSections = parentSegments(parentViews);
    parentLines = new T.InstancedMesh(
      new T.PlaneGeometry(1, 1),
      new T.MeshBasicMaterial({
        color: 0xffffff,
        side: T.DoubleSide,
        depthWrite: false,
      }),
      Math.max(1, parentSections.length),
    );
    parentLines.count = parentSections.length;
    parentLines.name = "parent-lines";
    parentLines.renderOrder = -1;
    const sectionTransform = new T.Object3D();
    const parentColor = new T.Color().setRGB(
      PARENT_LINE_BRIGHTNESS,
      PARENT_LINE_BRIGHTNESS,
      PARENT_LINE_BRIGHTNESS,
    );
    for (let i = 0; i < parentSections.length; i++) {
      const { start, end } = parentSections[i];
      sectionTransform.position.copy(start).lerp(end, 0.5);
      sectionTransform.rotation.z = Math.atan2(
        end.y - start.y,
        end.x - start.x,
      );
      sectionTransform.scale.set(start.distanceTo(end), PARENT_LINE_WIDTH, 1);
      sectionTransform.updateMatrix();
      parentLines.setMatrixAt(i, sectionTransform.matrix);
      parentLines.setColorAt(i, parentColor);
    }
    geometryGroup.add(parentLines);
    parentCorners = parentJoints(parentSections);
    parentLineJoints = new T.InstancedMesh(
      new T.CircleGeometry(PARENT_LINE_WIDTH / 2, 24),
      parentLines.material.clone(),
      Math.max(1, parentCorners.length),
    );
    parentLineJoints.count = parentCorners.length;
    parentLineJoints.name = "parent-line-joints";
    sectionTransform.rotation.set(0, 0, 0);
    sectionTransform.scale.set(1, 1, 1);
    for (let i = 0; i < parentCorners.length; i++) {
      sectionTransform.position.copy(parentCorners[i].position);
      sectionTransform.updateMatrix();
      parentLineJoints.setMatrixAt(i, sectionTransform.matrix);
      parentLineJoints.setColorAt(i, parentColor);
    }
    geometryGroup.add(parentLineJoints);
    const linePos: number[] = [],
      lineColors: number[] = [],
      lineEdges: (ConnectionPick | null)[] = [],
      layerList: { n: RenderNode; r: Region }[] = [];
    let regionLimit = 0;
    const addLine = (
      endpoint: T.Vector3Like,
      peer: T.Vector3Like,
      color: T.ColorRepresentation,
      edge: ConnectionPick | null = null,
    ) => {
      lineEdges.push(edge);
      linePos.push(endpoint.x, endpoint.y, endpoint.z, peer.x, peer.y, peer.z);
      const c = cachedColor(color);
      lineColors.push(c.r, c.g, c.b, c.r, c.g, c.b);
    };
    for (let i = 0; i < hullIds.length; i++) {
      const n = nodes.get(hullIds[i])!,
        p = n.pos,
        userColors = processColors(n);
      dummy.position.set(p.x, p.y, PROCESS_HEIGHT / 2);
      dummy.scale.set(1, 1, 1);
      dummy.updateMatrix();
      hull.setMatrixAt(i, dummy.matrix);
      dummy.position.set(p.x, p.y, -0.035);
      dummy.updateMatrix();
      baseGlow.setMatrixAt(i, dummy.matrix);
      baseGlow.setColorAt(i, cachedColor(0));
      dummy.position.z = -0.075;
      dummy.updateMatrix();
      haloGlow.setMatrixAt(i, dummy.matrix);
      haloGlow.setColorAt(i, cachedColor(0));
      for (const z of [0, 8])
        for (let j = 0; j < 4; j++) {
          const a = BOX_CORNERS[j],
            b = BOX_CORNERS[(j + 1) % 4];
          addLine(
            { x: p.x + a[0], y: p.y + a[1], z },
            { x: p.x + b[0], y: p.y + b[1], z },
            userColors.real,
          );
        }
      for (const x of [-1.1, 1.1])
        for (const y of [-1.1, 1.1])
          addLine(
            { x: p.x + x, y: p.y + y, z: 0 },
            { x: p.x + x, y: p.y + y, z: 8 },
            userColors.effective,
          );
      const stride =
        n.regions.length > 64 && hullIds[i] !== selectedProcess
          ? Math.ceil(n.regions.length / 64)
          : 1;
      for (let ri = 0; ri < n.regions.length; ri += stride) {
        if (regionLimit++ >= 120000) break;
        const first = n.regions[ri],
          last = n.regions[Math.min(ri + stride - 1, n.regions.length - 1)];
        layerList.push({ n, r: { ...first, h: last.z + last.h - first.z } });
      }
    }
    const layers = new T.InstancedMesh(
      new T.BoxGeometry(2.12, 2.12, 1),
      new T.MeshBasicMaterial({
        transparent: true,
        opacity: 0.18,
        depthWrite: true,
      }),
      Math.max(1, layerList.length),
    );
    layers.count = layerList.length;
    for (let i = 0; i < layerList.length; i++) {
      const { n, r } = layerList[i];
      dummy.position.set(n.pos.x, n.pos.y, r.z + r.h / 2);
      dummy.scale.set(1, 1, Math.max(0.0001, r.h));
      dummy.updateMatrix();
      layers.setMatrixAt(i, dummy.matrix);
      layers.setColorAt(i, cachedColor(regionColor(r)));
    }
    geometryGroup.add(layers);
    edgeViews = [];
    networkViews = [];
    portGroup.clear();
    geometryGroup.add(portGroup);
    const ports = portAllocator.allocate(input);
    const grouped = new Set(
      [...network.values()].flatMap((g) => g.members.map((e) => e.id)),
    );
    const visibleGroups = [...network.values()].filter(
      (g) =>
        visible.has(key(g.endpoint.process_id)) && networkPositions.has(g.id),
    );
    const markers = new T.InstancedMesh(
      new T.OctahedronGeometry(0.5),
      new T.MeshBasicMaterial({
        color: 0x68baff,
      }),
      Math.max(1, visibleGroups.length),
    );
    markers.count = visibleGroups.length;
    markers.userData.network = visibleGroups;
    geometryGroup.add(markers);
    const markerEdges = new T.InstancedMesh(
      new T.OctahedronGeometry(0.5),
      new T.MeshBasicMaterial({
        color: 0x8dd4ff,
        wireframe: true,
        transparent: true,
        opacity: 0.9,
      }),
      Math.max(1, visibleGroups.length),
    );
    markerEdges.count = markers.count;
    markerEdges.instanceMatrix = markers.instanceMatrix;
    geometryGroup.add(markerEdges);
    for (let i = 0; i < visibleGroups.length; i++) {
      const group = visibleGroups[i],
        p = networkPositions.get(group.id)!,
        pos = new T.Vector3(p.x, p.y, p.z);
      dummy.position.copy(pos);
      dummy.scale.set(1, 1, 1);
      dummy.updateMatrix();
      markers.setMatrixAt(i, dummy.matrix);
      let representative: T.CubicBezierCurve3 | undefined;
      const curves = new Map<string, T.CubicBezierCurve3>();
      const pick = {
        ...group,
        peer: null,
        shared: false,
        candidate: false,
        networkId: group.id,
      };
      for (const e of group.members) {
        const port = ports.get(ipcOwnerKey(e.endpoint))!;
        let curve = curves.get(port.id);
        if (!curve) {
          const start = port.position;
          curve = new T.CubicBezierCurve3(
            start,
            start.clone().addScaledVector(port.normal, 2),
            start
              .clone()
              .lerp(pos, 0.7)
              .add(new T.Vector3(0, 0, 2)),
            pos,
          );
          curves.set(port.id, curve);
          const pts = curve.getPoints(24);
          for (let j = 0; j < 24; j++)
            addLine(pts[j], pts[j + 1], 0x68baff, pick);
        }
        representative ??= curve;
        edgeViews.push({ e, curve, networkId: group.id });
      }
      if (representative)
        networkViews.push({ group, pos, curve: representative });
    }

    const mutedPos = [],
      mutedColors = [],
      mutedEdges = [];
    for (const e of snapshot.fd_relations) {
      if (grouped.has(e.id) || isKernelNetlink(e.socket)) continue;
      const a = nodes.get(key(e.endpoint.process_id)),
        b = e.peer && nodes.get(key(e.peer.process_id));
      if (
        !a ||
        !visible.has(key(a.identity)) ||
        (b && !visible.has(key(b.identity)))
      )
        continue;
      const sourcePort = ports.get(ipcOwnerKey(e.endpoint))!;
      const peerPort = b && e.peer ? ports.get(ipcOwnerKey(e.peer))! : null;
      const start = sourcePort.position;
      const end = peerPort
        ? peerPort.position
        : start
            .clone()
            .addScaledVector(sourcePort.normal, 3.5)
            .add(
              new T.Vector3(
                sourcePort.normal.y * 2.5,
                -sourcePort.normal.x * 2.5,
                0,
              ),
            );
      const curve = ipcCurve(
        start,
        end,
        sourcePort.normal,
        peerPort?.normal ?? sourcePort.normal.clone().negate(),
        a === b,
      );
      edgeViews.push({ e, curve });
      const pts = curve.getPoints(16);
      const mutedColor = cachedColor(
        e.shared ? SHARED_FD_COLOR : CANDIDATE_PEER_COLOR,
      );
      for (let j = 0; j < pts.length - 1; j++) {
        if (e.candidate || e.shared) {
          mutedPos.push(
            pts[j].x,
            pts[j].y,
            pts[j].z,
            pts[j + 1].x,
            pts[j + 1].y,
            pts[j + 1].z,
          );
          mutedColors.push(
            mutedColor.r,
            mutedColor.g,
            mutedColor.b,
            mutedColor.r,
            mutedColor.g,
            mutedColor.b,
          );
          mutedEdges.push(e);
        } else addLine(pts[j], pts[j + 1], e.peer ? 0x236857 : 0x24404c, e);
      }
    }
    ipcPorts = [...ports.values()];
    portStems = new T.InstancedMesh(
      new T.CylinderGeometry(
        IPC_PORT_RADIUS,
        IPC_PORT_RADIUS,
        IPC_STEM_LENGTH,
        8,
      ).rotateX(Math.PI / 2),
      new T.MeshBasicMaterial({ color: 0xffffff }),
      Math.max(1, ipcPorts.length),
    );
    portStems.name = "ipc-port-stems";
    portStems.count = ipcPorts.length;
    const transform = new T.Object3D(),
      axis = new T.Vector3(0, 0, 1);
    for (let i = 0; i < ipcPorts.length; i++) {
      const port = ipcPorts[i];
      transform.quaternion.setFromUnitVectors(axis, port.normal);
      transform.scale.set(port.radiusScale, port.radiusScale, 1);
      transform.position
        .copy(port.position)
        .addScaledVector(port.normal, -IPC_STEM_LENGTH / 2);
      transform.updateMatrix();
      portStems.setMatrixAt(i, transform.matrix);
      portStems.setColorAt(i, cachedColor(IPC_PORT_COLOR));
    }
    portGroup.add(portStems);
    const lines = new T.BufferGeometry();
    lines.setAttribute("position", new T.Float32BufferAttribute(linePos, 3));
    lines.setAttribute("color", new T.Float32BufferAttribute(lineColors, 3));
    const solidLines = new T.LineSegments(
      lines,
      new T.LineBasicMaterial({
        vertexColors: true,
        transparent: true,
        opacity: 0.75,
        depthWrite: false,
        blending: T.AdditiveBlending,
      }),
    );
    solidLines.userData.fd_relations = lineEdges;
    geometryGroup.add(solidLines);
    const muted = new T.BufferGeometry();
    muted.setAttribute("position", new T.Float32BufferAttribute(mutedPos, 3));
    muted.setAttribute("color", new T.Float32BufferAttribute(mutedColors, 3));
    const mutedLines = new T.LineSegments(
      muted,
      new T.LineBasicMaterial({
        vertexColors: true,
        transparent: true,
        opacity: 0.3,
        depthWrite: false,
      }),
    );
    mutedLines.userData.fd_relations = mutedEdges;
    geometryGroup.add(mutedLines);
    buildHardware(input);
    refreshFileScene(input);
  }
  function hardwareStyle(device: HardwareDevice) {
    if (device.kind) return HARDWARE_STYLE[device.kind];
    return device.role === "host_bridge"
      ? HARDWARE_STYLE.host_bridge
      : HARDWARE_STYLE.port;
  }
  /** Devices, bridges and PCIe ribbons of the hardware bay in front of the processes. */
  function buildHardware(input: SceneInput) {
    hardwareInput = input.hardware;
    hardwareViews = [];
    const hardware = input.hardware;
    if (!hardware?.devices.length) return;
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
    edgeGeometry.setAttribute(
      "position",
      new T.Float32BufferAttribute(edges, 3),
    );
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
  }
  /**
   * One curve per process and device it holds, from the process's front face to the
   * device roof. `visible` limits the processes; all are used when it is omitted.
   */
  function userLinks(
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
  function updateHardwareOverlay(selectionState: SelectionState) {
    hardwareOverlay.traverse((object) =>
      disposeObject(object, retiredMaterials),
    );
    scene.remove(hardwareOverlay);
    hardwareOverlay = new T.Group();
    hardwareOverlay.name = "hardware-overlay";
    scene.add(hardwareOverlay);
    hardwarePathViews = [];
    const hardware = hardwareInput;
    if (!hardware) return;
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
    for (const { process, address, curve, color } of userLinks(hardware))
      if (selected === address || selectionState.process === process)
        line(curve.getPoints(16), color);
    if (!positions.length) return;
    const geometry = new T.BufferGeometry();
    geometry.setAttribute(
      "position",
      new T.Float32BufferAttribute(positions, 3),
    );
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
  }
  function refreshFileScene(input: FileSceneInput) {
    syncNodes(input);
    const {
      files: recentFiles,
      filePositions,
      netlink,
      netlinkPositions,
    } = input;
    fileGroup.traverse((object) => disposeObject(object, retiredMaterials));
    scene.remove(fileGroup);
    fileGroup = new T.Group();
    scene.add(fileGroup);
    const visible = input.visible,
      files = [...recentFiles.values()].filter((f) =>
        visible.has(key(f.process_id)),
      );
    const markers = new T.InstancedMesh(
      new T.BoxGeometry(1.5, 0.22, 1.8),
      new T.MeshBasicMaterial({
        color: 0xe2d5a0,
        transparent: true,
        opacity: 0.65,
      }),
      Math.max(1, files.length),
    );
    markers.count = files.length;
    markers.userData.files = files;
    fileGroup.add(markers);
    fileViews = new Map();
    const vertices = [],
      paths = [];
    for (let i = 0; i < files.length; i++) {
      const file = files[i],
        p = filePositions.get(file.id)!,
        owner = nodes.get(key(file.process_id))!,
        pos = new T.Vector3(p.x, p.y, p.z);
      dummy.position.copy(pos);
      dummy.scale.set(1, 1, 1);
      dummy.updateMatrix();
      markers.setMatrixAt(i, dummy.matrix);
      const start = owner.pos.clone().setZ(0.5),
        mid = start.clone().lerp(pos, 0.5);
      mid.y -= 1.5;
      const curve = new T.QuadraticBezierCurve3(start, mid, pos);
      fileViews.set(file.id, { file, pos, curve });
      const points = curve.getPoints(16);
      for (let j = 0; j < 16; j++) {
        vertices.push(...points[j], ...points[j + 1]);
        paths.push(file.id);
      }
    }
    const geometry = new T.BufferGeometry();
    geometry.setAttribute(
      "position",
      new T.Float32BufferAttribute(vertices, 3),
    );
    const lines = new T.LineSegments(
      geometry,
      new T.LineBasicMaterial({
        color: 0xc9bb88,
        transparent: true,
        opacity: 0.5,
        depthWrite: false,
      }),
    );
    lines.userData.filePaths = paths;
    fileGroup.add(lines);
    netlinkViews = new Map();
    const sockets = [...netlink.values()].filter(
      (socket) =>
        visible.has(key(socket.endpoint.process_id)) &&
        netlinkPositions.has(socket.id),
    );
    const plateMaterial = new T.MeshBasicMaterial({
      color: 0xf5a45d,
      transparent: true,
      opacity: 0.85,
    });
    const plates = new T.InstancedMesh(
      new T.CylinderGeometry(0.85, 0.85, 0.16, 6).rotateX(Math.PI / 2),
      [
        new T.MeshBasicMaterial({ color: 0xad6c38 }),
        plateMaterial,
        new T.MeshBasicMaterial({ color: 0xe49252 }),
      ],
      Math.max(1, sockets.length),
    );
    plates.count = sockets.length;
    plates.userData.netlink = sockets;
    fileGroup.add(plates);
    const socketVertices: number[] = [],
      socketPaths: string[] = [];
    for (let i = 0; i < sockets.length; i++) {
      const socket = sockets[i],
        p = netlinkPositions.get(socket.id)!,
        owner = nodes.get(key(socket.endpoint.process_id))!,
        pos = new T.Vector3(p.x, p.y, p.z);
      dummy.position.copy(pos);
      dummy.rotation.set(0, 0, 0);
      dummy.scale.set(1, 1, 1);
      dummy.updateMatrix();
      plates.setMatrixAt(i, dummy.matrix);
      const start = owner.pos.clone().setZ(0.5),
        mid = start.clone().lerp(pos, 0.5);
      mid.y -= 1.5;
      const curve = new T.QuadraticBezierCurve3(start, mid, pos);
      netlinkViews.set(socket.id, { socket, pos, curve });
      const points = curve.getPoints(16);
      for (let j = 0; j < 16; j++) {
        socketVertices.push(...points[j], ...points[j + 1]);
        socketPaths.push(socket.id);
      }
    }
    dummy.rotation.set(0, 0, 0);
    const socketGeometry = new T.BufferGeometry();
    socketGeometry.setAttribute(
      "position",
      new T.Float32BufferAttribute(socketVertices, 3),
    );
    const socketLines = new T.LineSegments(
      socketGeometry,
      new T.LineBasicMaterial({
        color: 0xf5a45d,
        transparent: true,
        opacity: 0.55,
        depthWrite: false,
      }),
    );
    socketLines.userData.netlinkPaths = socketPaths;
    fileGroup.add(socketLines);
  }
  function updateParentSelection(selectionState: SelectionState) {
    if (!parentLines?.instanceColor) return;
    const active = new Set<string>();
    if (selectionState.process) {
      let child = selectionState.process;
      const seen = new Set();
      while (child && !seen.has(child)) {
        seen.add(child);
        const parentId = nodes.get(child)?.parent_id;
        const parent: string | null | undefined = parentId && key(parentId);
        if (!parent || parent === child || !nodes.has(parent)) break;
        active.add(`${parent}>${child}`);
        child = parent;
      }
      const children = new Map<string, string[]>();
      for (const [id, n] of nodes) {
        const parent = n.parent_id && key(n.parent_id);
        if (parent && parent !== id) {
          const list = children.get(parent) ?? [];
          list.push(id);
          children.set(parent, list);
        }
      }
      const pending = [selectionState.process];
      const descendants = new Set<string>();
      while (pending.length) {
        const parent = pending.pop()!;
        if (descendants.has(parent)) continue;
        descendants.add(parent);
        for (const child of children.get(parent) ?? []) {
          active.add(`${parent}>${child}`);
          pending.push(child);
        }
      }
    }
    for (const path of parentViews)
      path.active = active.has(`${path.parent}>${path.child}`);
    const color = new T.Color();
    for (const [mesh, sections] of [
      [parentLines, parentSections],
      [parentLineJoints, parentCorners],
    ] as const) {
      if (!mesh?.instanceColor) continue;
      for (let i = 0; i < sections.length; i++) {
        const bright = sections[i].paths.some(
          (index) => parentViews[index].active,
        );
        const level = bright ? 1 : PARENT_LINE_BRIGHTNESS;
        mesh.setColorAt(i, color.setRGB(level, level, level));
      }
      mesh.instanceColor.needsUpdate = true;
    }
  }
  function updateSelection(selectionState: SelectionState) {
    updateHardwareOverlay(selectionState);
    const n = nodes.get(selectionState.process ?? "");
    selection.visible = !!n;
    if (n) selection.position.set(n.pos.x, n.pos.y, 4);
    const view = selectionState.netlink
      ? netlinkViews.get(selectionState.netlink)
      : selectionState.file
        ? fileViews.get(selectionState.file)
        : selectionState.network
          ? networkViews.find((v) => v.group.id === selectionState.network)
          : edgeViews.find((v) => v.e.id === selectionState.connection);
    edgeSelection.visible = !!view;
    if (view) {
      const curves = selectionState.network
        ? [
            ...new Set(
              edgeViews
                .filter((edge) => edge.networkId === selectionState.network)
                .map((edge) => edge.curve),
            ),
          ]
        : [view.curve];
      const points: T.Vector3[] = [];
      for (const curve of curves) {
        const samples = curve.getPoints(32);
        for (let i = 1; i < samples.length; i++)
          points.push(samples[i - 1], samples[i]);
      }
      edgeSelection.geometry.dispose();
      edgeSelection.geometry = new T.BufferGeometry().setFromPoints(points);
    }
    if (portStems?.instanceColor) {
      const color = new T.Color();
      ipcPorts.forEach((port, i) => {
        const active =
          (selectionState.connection !== null &&
            port.connections.has(selectionState.connection)) ||
          (selectionState.network !== null &&
            port.networks.has(selectionState.network));
        portStems!.setColorAt(i, color.set(active ? 0xc9fff2 : IPC_PORT_COLOR));
      });
      portStems.instanceColor.needsUpdate = true;
    }
    updateParentSelection(selectionState);
  }
  function cpuGlowVisual(id: string) {
    const index = hullIds.indexOf(id);
    if (index < 0 || !baseGlow) return null;
    const color = new T.Color();
    baseGlow.getColorAt(index, color);
    return { r: color.r, g: color.g, b: color.b };
  }
  function parentLineVisual(parent: string, child: string) {
    const index = parentViews.findIndex(
      (view) => view.parent === parent && view.child === child,
    );
    if (index < 0 || !parentLines) return null;
    const level = parentViews[index].active ? 1 : PARENT_LINE_BRIGHTNESS;
    return {
      r: level,
      g: level,
      b: level,
      width: PARENT_LINE_WIDTH,
      points: parentViews[index].points.map((point) => point.toArray()),
    };
  }

  return {
    root: scene,
    rebuild: buildScene,
    refreshFiles: refreshFileScene,
    updateSelection,
    cpuGlowVisual,
    parentLineVisual,
    setFogDensity(density: number) {
      (scene.fog as T.FogExp2).density = density;
    },
    renderView(): RenderView {
      return {
        nodes,
        hullIds,
        networkViews,
        fileViews,
        netlinkViews,
        hardwareViews,
        hardwarePathViews,
        edgeViews,
        baseGlow,
        haloGlow,
      };
    },
    pickingView(): PickingView {
      return {
        nodes,
        hullIds,
        hull,
        objects: [
          ...geometryGroup.children.filter(
            (object) =>
              object !== parentLines &&
              object !== parentLineJoints &&
              object !== portGroup,
          ),
          ...fileGroup.children,
        ],
      };
    },
    dispose() {
      scene.traverse((object) => disposeObject(object));
      releaseRetiredMaterials();
    },
  };
}
export type SpaceScene = ReturnType<typeof createSpaceScene>;
