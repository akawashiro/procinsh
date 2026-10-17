// Build and update Three.js geometry from data and selection state.
import * as T from "three";
import { isKernelNetlink } from "./netlink.js";
import { key, processColors, PROCESS_SIDE } from "./model.js";
import type {
  Region,
  RenderNode,
  EdgeView,
  NetworkView,
  FileView,
  NetlinkView,
  ConnectionPick,
  SelectionState,
} from "./types.js";
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
const SOCKET_HEIGHT = 4;
const SHARED_FD_COLOR = 0x54828b;
const CANDIDATE_PEER_COLOR = 0xc69a50;

// Attach to the side facing the peer, with a small gap outside the process hull.
function socketFaceOffset(position: T.Vector3, peer: T.Vector3): T.Vector3 {
  const dx = peer.x - position.x,
    dy = peer.y - position.y,
    offset = PROCESS_SIDE / 2 + 0.02;
  return Math.abs(dx) >= Math.abs(dy)
    ? new T.Vector3(Math.sign(dx) * offset, 0, 0)
    : new T.Vector3(0, Math.sign(dy) * offset, 0);
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

function disposeObject(
  object: T.Object3D,
  retainedMaterials?: ReadonlySet<T.Material>,
) {
  if (
    object instanceof T.Mesh ||
    object instanceof T.Line ||
    object instanceof T.Points
  ) {
    if (object instanceof T.InstancedMesh) object.dispose();
    object.geometry.dispose();
    const materials = Array.isArray(object.material)
      ? object.material
      : [object.material];
    for (const material of materials)
      if (!retainedMaterials?.has(material)) material.dispose();
  }
}

export function createSpaceScene() {
  const scene = new T.Scene();
  scene.fog = new T.FogExp2(0x03090e, 0.0015);
  let nodes = new Map<string, RenderNode>();
  let edgeViews: EdgeView[] = [],
    networkViews: NetworkView[] = [],
    fileViews = new Map<string, FileView>(),
    netlinkViews = new Map<string, NetlinkView>();
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
  // Materials outlive geometry rebuilds so the renderer can retain shader programs.
  const materials = {
    hull: new T.MeshBasicMaterial({
      color: 0x4bd6bf,
      transparent: true,
      opacity: 0.025,
      depthWrite: false,
    }),
    baseGlow: new T.MeshBasicMaterial({
      color: 0xffffff,
      transparent: true,
      opacity: 0.95,
      blending: T.AdditiveBlending,
      depthWrite: false,
    }),
    haloGlow: new T.MeshBasicMaterial({
      color: 0xffffff,
      transparent: true,
      opacity: 0.32,
      blending: T.AdditiveBlending,
      depthWrite: false,
    }),
    parentLines: new T.MeshBasicMaterial({
      color: 0xffffff,
      side: T.DoubleSide,
      depthWrite: false,
    }),
    layers: new T.MeshBasicMaterial({
      transparent: true,
      opacity: 0.18,
      depthWrite: true,
    }),
    networkMarkers: new T.MeshBasicMaterial({
      color: 0x68baff,
      transparent: true,
      opacity: 0.2,
    }),
    networkEdges: new T.MeshBasicMaterial({
      color: 0x8dd4ff,
      wireframe: true,
      transparent: true,
      opacity: 0.9,
    }),
    connections: new T.LineBasicMaterial({
      vertexColors: true,
      transparent: true,
      opacity: 0.75,
      depthWrite: false,
      blending: T.AdditiveBlending,
    }),
    mutedConnections: new T.LineBasicMaterial({
      vertexColors: true,
      transparent: true,
      opacity: 0.3,
      depthWrite: false,
    }),
    fileMarkers: new T.MeshBasicMaterial({
      color: 0xe2d5a0,
      transparent: true,
      opacity: 0.65,
    }),
    fileConnections: new T.LineBasicMaterial({
      color: 0xc9bb88,
      transparent: true,
      opacity: 0.5,
      depthWrite: false,
    }),
    netlinkPlates: new T.MeshBasicMaterial({
      color: 0xf5a45d,
      transparent: true,
      opacity: 0.85,
    }),
    netlinkSides: new T.MeshBasicMaterial({ color: 0xad6c38 }),
    netlinkBottom: new T.MeshBasicMaterial({ color: 0xe49252 }),
    netlinkConnections: new T.LineBasicMaterial({
      color: 0xf5a45d,
      transparent: true,
      opacity: 0.55,
      depthWrite: false,
    }),
  };
  const sharedMaterials = new Set<T.Material>(Object.values(materials));
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
  const edgeSelection = new T.Line(
    new T.BufferGeometry(),
    new T.LineBasicMaterial({
      color: 0xc9fff2,
      transparent: true,
      opacity: 1,
      depthWrite: false,
      blending: T.AdditiveBlending,
    }),
  );
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
    geometryGroup.traverse((object) => disposeObject(object, sharedMaterials));
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
      materials.hull,
      Math.max(1, hullIds.length),
    );
    hull.count = hullIds.length;
    geometryGroup.add(hull);
    baseGlow = new T.InstancedMesh(
      new T.BoxGeometry(2.28, 2.28, 0.07),
      materials.baseGlow,
      Math.max(1, hullIds.length),
    );
    baseGlow.count = hullIds.length;
    baseGlow.frustumCulled = false;
    geometryGroup.add(baseGlow);
    haloGlow = new T.InstancedMesh(
      new T.BoxGeometry(3.15, 3.15, 0.025),
      materials.haloGlow,
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
      materials.parentLines,
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
      materials.parentLines,
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
      endpoint: number[],
      peer: number[],
      color: T.ColorRepresentation,
      edge: ConnectionPick | null = null,
    ) => {
      lineEdges.push(edge);
      linePos.push(...endpoint, ...peer);
      const c = new T.Color(color);
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
      baseGlow.setColorAt(i, new T.Color(0));
      dummy.position.z = -0.075;
      dummy.updateMatrix();
      haloGlow.setMatrixAt(i, dummy.matrix);
      haloGlow.setColorAt(i, new T.Color(0));
      for (const z of [0, 8])
        for (let j = 0; j < 4; j++) {
          const corners = [
              [-1.1, -1.1],
              [1.1, -1.1],
              [1.1, 1.1],
              [-1.1, 1.1],
            ],
            a = corners[j],
            b = corners[(j + 1) % 4];
          addLine(
            [p.x + a[0], p.y + a[1], z],
            [p.x + b[0], p.y + b[1], z],
            userColors.real,
          );
        }
      for (const x of [-1.1, 1.1])
        for (const y of [-1.1, 1.1])
          addLine(
            [p.x + x, p.y + y, 0],
            [p.x + x, p.y + y, 8],
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
      materials.layers,
      Math.max(1, layerList.length),
    );
    layers.count = layerList.length;
    for (let i = 0; i < layerList.length; i++) {
      const { n, r } = layerList[i];
      dummy.position.set(n.pos.x, n.pos.y, r.z + r.h / 2);
      dummy.scale.set(1, 1, Math.max(0.0001, r.h));
      dummy.updateMatrix();
      layers.setMatrixAt(i, dummy.matrix);
      layers.setColorAt(i, new T.Color(regionColor(r)));
    }
    geometryGroup.add(layers);
    edgeViews = [];
    networkViews = [];
    const grouped = new Set(
      [...network.values()].flatMap((g) => g.members.map((e) => e.id)),
    );
    const visibleGroups = [...network.values()].filter(
      (g) =>
        visible.has(key(g.endpoint.process_id)) && networkPositions.has(g.id),
    );
    const markers = new T.InstancedMesh(
      new T.OctahedronGeometry(0.75),
      materials.networkMarkers,
      Math.max(1, visibleGroups.length),
    );
    markers.count = visibleGroups.length;
    markers.userData.network = visibleGroups;
    geometryGroup.add(markers);
    const markerEdges = new T.InstancedMesh(
      new T.OctahedronGeometry(0.75),
      materials.networkEdges,
      Math.max(1, visibleGroups.length),
    );
    markerEdges.count = markers.count;
    markerEdges.instanceMatrix = markers.instanceMatrix;
    geometryGroup.add(markerEdges);
    for (let i = 0; i < visibleGroups.length; i++) {
      const group = visibleGroups[i],
        p = networkPositions.get(group.id)!,
        pos = new T.Vector3(p.x, p.y, p.z),
        owner = nodes.get(key(group.endpoint.process_id))!;
      dummy.position.copy(pos);
      dummy.scale.set(1, 1, 1);
      dummy.updateMatrix();
      markers.setMatrixAt(i, dummy.matrix);
      const start = owner.pos.clone().setZ(PROCESS_HEIGHT),
        mid = start.clone().lerp(pos, 0.5);
      mid.z += 2;
      const curve = new T.QuadraticBezierCurve3(start, mid, pos);
      networkViews.push({ group, pos, curve });
      for (const e of group.members)
        edgeViews.push({ e, curve, networkId: group.id });
      const pts = curve.getPoints(24),
        pick = {
          ...group,
          peer: null,
          shared: false,
          candidate: false,
          networkId: group.id,
        };
      for (let j = 0; j < 24; j++)
        addLine(pts[j].toArray(), pts[j + 1].toArray(), 0x68baff, pick);
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
      const socket = e.endpoint.resource.kind === "socket";
      const faceOffset =
        socket && b && a !== b
          ? socketFaceOffset(a.pos, b.pos)
          : new T.Vector3(1.12, 0, 0);
      // A self-connection keeps each FD on its own opposing face across reversals.
      if (socket && a === b && e.endpoint.fd > e.peer!.fd) faceOffset.negate();
      const height = (fd: number) =>
        socket
          ? SOCKET_HEIGHT + ((fd % 12) - 5.5) * 0.11
          : 1 + (fd % 12) * 0.11;
      const start = a.pos
        .clone()
        .add(faceOffset)
        .add(new T.Vector3(0, 0, height(e.endpoint.fd)));
      const end = b
        ? b.pos
            .clone()
            .sub(faceOffset)
            .add(new T.Vector3(0, 0, height(e.peer!.fd)))
        : start.clone().add(new T.Vector3(3.5, -2.5, socket ? 0 : -0.6));
      const mid = start.clone().lerp(end, 0.5);
      if (!socket) mid.z = 0.22;
      const curve = new T.QuadraticBezierCurve3(start, mid, end);
      edgeViews.push({ e, curve });
      const pts = curve.getPoints(16);
      const mutedColor = new T.Color(
        e.shared ? SHARED_FD_COLOR : CANDIDATE_PEER_COLOR,
      );
      for (let j = 0; j < 16; j++) {
        if (e.candidate || e.shared) {
          mutedPos.push(...pts[j], ...pts[j + 1]);
          mutedColors.push(
            mutedColor.r,
            mutedColor.g,
            mutedColor.b,
            mutedColor.r,
            mutedColor.g,
            mutedColor.b,
          );
          mutedEdges.push(e);
        } else
          addLine(
            pts[j].toArray(),
            pts[j + 1].toArray(),
            e.peer ? 0x236857 : 0x24404c,
            e,
          );
      }
    }
    const lines = new T.BufferGeometry();
    lines.setAttribute("position", new T.Float32BufferAttribute(linePos, 3));
    lines.setAttribute("color", new T.Float32BufferAttribute(lineColors, 3));
    const solidLines = new T.LineSegments(lines, materials.connections);
    solidLines.userData.fd_relations = lineEdges;
    geometryGroup.add(solidLines);
    const muted = new T.BufferGeometry();
    muted.setAttribute("position", new T.Float32BufferAttribute(mutedPos, 3));
    muted.setAttribute("color", new T.Float32BufferAttribute(mutedColors, 3));
    const mutedLines = new T.LineSegments(muted, materials.mutedConnections);
    mutedLines.userData.fd_relations = mutedEdges;
    geometryGroup.add(mutedLines);
    refreshFileScene(input);
  }
  function refreshFileScene(input: FileSceneInput) {
    syncNodes(input);
    const {
      files: recentFiles,
      filePositions,
      netlink,
      netlinkPositions,
    } = input;
    fileGroup.traverse((object) => disposeObject(object, sharedMaterials));
    scene.remove(fileGroup);
    fileGroup = new T.Group();
    scene.add(fileGroup);
    const visible = input.visible,
      files = [...recentFiles.values()].filter((f) =>
        visible.has(key(f.process_id)),
      );
    const markers = new T.InstancedMesh(
      new T.BoxGeometry(1.5, 0.22, 1.8),
      materials.fileMarkers,
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
    const lines = new T.LineSegments(geometry, materials.fileConnections);
    lines.userData.filePaths = paths;
    fileGroup.add(lines);
    netlinkViews = new Map();
    const sockets = [...netlink.values()].filter(
      (socket) =>
        visible.has(key(socket.endpoint.process_id)) &&
        netlinkPositions.has(socket.id),
    );
    const plates = new T.InstancedMesh(
      new T.CylinderGeometry(0.85, 0.85, 0.16, 6).rotateX(Math.PI / 2),
      [
        materials.netlinkSides,
        materials.netlinkPlates,
        materials.netlinkBottom,
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
      materials.netlinkConnections,
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
    if (view) edgeSelection.geometry.setFromPoints(view.curve.getPoints(32));
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
            (object) => object !== parentLines && object !== parentLineJoints,
          ),
          ...fileGroup.children,
        ],
      };
    },
    dispose() {
      scene.traverse((object) => disposeObject(object, sharedMaterials));
      for (const material of sharedMaterials) material.dispose();
    },
  };
}
export type SpaceScene = ReturnType<typeof createSpaceScene>;
