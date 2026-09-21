import * as T from "/vendor/three.module.js";
import { OrbitControls } from "/vendor/OrbitControls.js";
import {
  RecentFiles,
  fileKey,
  fileLayout,
  processColors,
  remoteLabel,
  key,
  layoutMaps,
  addressZ,
  edgeDirection,
  ipcParticlePlan,
  cpuGlowLevel,
  treeLayout,
  stableLayout,
  networkGroups,
  networkLayout,
  connectionState,
} from "/space-model.js";
import type {
  SpaceNode,
  Topology,
  Edge,
  Port,
  SocketEndpoint,
  SpaceActivity,
  Lease,
  ProcessSummary,
} from "./api-types.js";
import type {
  Position,
  Region,
  NetworkGroup,
  RecentFile,
  CpuGlow,
} from "./space-model.js";
import type { SpaceElements } from "./dom-types.js";
interface RenderNode extends SpaceNode {
  pos: T.Vector3;
  regions: Region[];
  invalidated?: boolean;
}
interface EdgeView {
  e: Edge;
  curve: T.QuadraticBezierCurve3;
  networkId?: string;
}
interface NetworkView {
  group: NetworkGroup;
  pos: T.Vector3;
  curve: T.QuadraticBezierCurve3;
}
interface FileView {
  file: RecentFile;
  pos: T.Vector3;
  curve: T.QuadraticBezierCurve3;
}
interface EdgeStat {
  bytes: number;
  count: number;
  time: number;
}
type ConnectionPick = Edge & { networkId?: string };
type PickResult = ConnectionPick | { fileId: string };
type Particle = {
  start: number;
  duration: number;
  color: number;
  strength?: number;
  memory?: boolean;
  networkId?: string;
  fileId?: string;
} & (
  | { curve: T.QuadraticBezierCurve3; direction: number | null; pos?: never }
  | { pos: T.Vector3; curve?: never; direction?: never }
);
// Three.js userData is an untyped extension point. Only these picking fields are
// written by this module; narrow at the boundary instead of leaking it into UI state.
interface PickData {
  files?: RecentFile[];
  filePaths?: string[];
  network?: NetworkGroup[];
  edges?: (ConnectionPick | null)[];
}
function pickData(object: T.Object3D): PickData {
  return object.userData as PickData;
}
function disposeObject(object: T.Object3D) {
  if (
    object instanceof T.Mesh ||
    object instanceof T.Line ||
    object instanceof T.Points
  ) {
    object.geometry.dispose();
    const materials = Array.isArray(object.material)
      ? object.material
      : [object.material];
    for (const material of materials) material.dispose();
  }
}
function $<K extends keyof SpaceElements>(id: K): SpaceElements[K] {
  const element = document.getElementById(id);
  if (!element) throw new Error(`Missing element: ${id}`);
  return element as SpaceElements[K];
}
function context2d(canvas: HTMLCanvasElement): CanvasRenderingContext2D {
  const context = canvas.getContext("2d");
  if (!context) throw new Error("Canvas 2D is unavailable");
  return context;
}
const canvas = $("world"),
  labelCanvas = $("labels"),
  labelContext = context2d(labelCanvas);
const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
let renderScale = Math.min(devicePixelRatio, 1.5),
  slowFrames = 0;
let renderer: T.WebGLRenderer;
try {
  renderer = new T.WebGLRenderer({ canvas, antialias: false, alpha: false });
} catch (e) {
  $("failure").hidden = false;
  $("failure").textContent =
    "WebGL2 is unavailable. Use Back to process list to return to the process list.";
  throw e;
}
renderer.setPixelRatio(renderScale);
renderer.setSize(innerWidth, innerHeight);
renderer.setClearColor(0x03090e);
renderer.outputColorSpace = T.SRGBColorSpace;
const scene = new T.Scene();
scene.fog = new T.FogExp2(0x03090e, 0.0015);
const camera = new T.PerspectiveCamera(45, innerWidth / innerHeight, 0.1, 1500);
camera.up.set(0, 0, 1);
camera.position.set(65, -85, 70);
const controls = new OrbitControls(camera, canvas);
controls.enableDamping = true;
controls.dampingFactor = 0.07;
controls.maxPolarAngle = Math.PI * 0.49;
controls.minDistance = 3;
controls.maxDistance = 500;
// OrbitControls scales panning by camera distance. Keep close-up navigation usable.
function updatePanSpeed() {
  controls.panSpeed = Math.max(
    1,
    40 /
      Math.max(
        controls.minDistance,
        camera.position.distanceTo(controls.target),
      ),
  );
}
controls.addEventListener("start", updatePanSpeed);
controls.addEventListener("change", updatePanSpeed);

const nodes = new Map<string, RenderNode>(),
  cpuGlows = new Map<string, CpuGlow>();
let topology: Topology = { nodes: [], edges: [] },
  edgeViews: EdgeView[] = [],
  parentViews: { parent: string; child: string }[] = [],
  parentLines: T.LineSegments<T.BufferGeometry, T.LineBasicMaterial> | null =
    null,
  edgeStats = new Map<string, EdgeStat>(),
  selected: string | null = null,
  selectedEdge: string | null = null,
  geometryGroup = new T.Group();
scene.add(geometryGroup);
let network = new Map<string, NetworkGroup>(),
  networkPositions = new Map<string, Position>(),
  networkViews: NetworkView[] = [],
  selectedNetwork: string | null = null,
  hoveredNetwork: string | null = null;
const recentFiles = new RecentFiles();
let filePositions = new Map<string, Position>(),
  fileViews = new Map<string, FileView>(),
  fileGroup = new T.Group(),
  selectedFile: string | null = null,
  hoveredFile: string | null = null,
  lastFilePrune = 0;
scene.add(fileGroup);
let firstView = true;
let hull: T.InstancedMesh | null = null,
  baseGlow: T.InstancedMesh | null = null,
  haloGlow: T.InstancedMesh | null = null,
  hullIds: string[] = [],
  particles: Particle[] = [],
  lastTime = performance.now(),
  frames = 0,
  frameTime = lastTime;
const CAP = 8192,
  positions = new Float32Array(CAP * 3),
  colors = new Float32Array(CAP * 3);
const pg = new T.BufferGeometry();
pg.setAttribute("position", new T.BufferAttribute(positions, 3));
pg.setAttribute("color", new T.BufferAttribute(colors, 3));
pg.setDrawRange(0, 0);
const particleCanvas = document.createElement("canvas");
particleCanvas.width = particleCanvas.height = 64;
const particleContext = context2d(particleCanvas);
particleContext.fillStyle = "#fff";
particleContext.beginPath();
particleContext.arc(32, 32, 30, 0, Math.PI * 2);
particleContext.fill();
const particleTexture = new T.CanvasTexture(particleCanvas);
const points = new T.Points(
  pg,
  new T.PointsMaterial({
    map: particleTexture,
    alphaTest: 0.1,
    size: 0.4,
    vertexColors: true,
    transparent: true,
    opacity: 0.95,
    blending: T.AdditiveBlending,
    depthWrite: false,
    sizeAttenuation: true,
  }),
);
points.frustumCulled = false;
scene.add(points);
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
const ray = new T.Raycaster(),
  mouse = new T.Vector2(),
  dummy = new T.Object3D();
let down: { x: number; y: number } | undefined;
const labelPoint = new T.Vector3();
function resizeLabels() {
  const ratio = Math.min(devicePixelRatio, 2);
  labelCanvas.width = Math.round(innerWidth * ratio);
  labelCanvas.height = Math.round(innerHeight * ratio);
  labelContext.setTransform(ratio, 0, 0, ratio, 0, 0);
}
function drawLabels() {
  labelContext.clearRect(0, 0, innerWidth, innerHeight);
  labelContext.font = "10px ui-monospace, SFMono-Regular, Consolas, monospace";
  labelContext.textAlign = "center";
  labelContext.textBaseline = "bottom";
  labelContext.lineJoin = "round";
  labelContext.lineWidth = 3;
  const labels: {
    name: string;
    x: number;
    y: number;
    depth: number;
    active: boolean;
    external: boolean;
  }[] = [];
  const add = (
    name: string,
    pos: T.Vector3,
    active: boolean,
    external = false,
  ) => {
    labelPoint.copy(pos).project(camera);
    if (labelPoint.z < -1 || labelPoint.z > 1) return;
    const x = (labelPoint.x * 0.5 + 0.5) * innerWidth,
      y = (-labelPoint.y * 0.5 + 0.5) * innerHeight - 3;
    if (x < -200 || x > innerWidth + 200 || y < 0 || y > innerHeight + 12)
      return;
    labels.push({ name, x, y, depth: labelPoint.z, active, external });
  };
  for (const id of hullIds) {
    const n = nodes.get(id);
    if (n) add(n.name, n.pos.clone().setZ(8.55), id === selected);
  }
  for (const v of networkViews)
    add(
      v.group.label,
      v.pos.clone().add(new T.Vector3(0, 0, 1)),
      v.group.id === selectedNetwork ||
        v.group.id === hoveredNetwork ||
        v.group.members.some((e) => e.id === selectedEdge),
      true,
    );
  for (const v of fileViews.values())
    add(
      v.file.label,
      v.pos.clone().add(new T.Vector3(0, 0, 0.7)),
      v.file.id === selectedFile || v.file.id === hoveredFile,
      true,
    );
  labels.sort(
    (a, b) => Number(b.active) - Number(a.active) || a.depth - b.depth,
  );
  const occupied: {
    left: number;
    right: number;
    top: number;
    bottom: number;
  }[] = [];
  for (const label of labels) {
    const max = label.external ? 300 : 130,
      width = Math.min(max, labelContext.measureText(label.name).width) + 6;
    const box = {
      left: label.x - width / 2,
      right: label.x + width / 2,
      top: label.y - 12,
      bottom: label.y + 2,
    };
    if (
      !label.active &&
      occupied.some(
        (o) =>
          box.left < o.right &&
          box.right > o.left &&
          box.top < o.bottom &&
          box.bottom > o.top,
      )
    )
      continue;
    occupied.push(box);
    labelContext.strokeStyle = "rgba(2,9,14,.92)";
    labelContext.strokeText(label.name, label.x, label.y, max);
    labelContext.fillStyle = label.active
      ? "#e0faff"
      : label.external
        ? "#84caff"
        : "rgba(190,246,232,.88)";
    labelContext.fillText(label.name, label.x, label.y, max);
  }
}
resizeLabels();
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
function worldBounds() {
  const box = new T.Box3();
  for (const n of nodes.values()) {
    box.expandByPoint(n.pos);
    box.expandByPoint(n.pos.clone().setZ(8));
  }
  for (const p of networkPositions.values())
    box.expandByPoint(new T.Vector3(p.x, p.y, p.z + 1));
  for (const p of filePositions.values())
    box.expandByPoint(new T.Vector3(p.x, p.y, p.z - 1));
  return box;
}
function adaptWorld() {
  if (!nodes.size) return;
  const size = worldBounds().getSize(new T.Vector3()),
    extent = Math.max(50, size.x, size.y, size.z);
  controls.maxDistance = Math.max(500, extent * 2);
  camera.far = Math.max(1500, extent * 4);
  camera.updateProjectionMatrix();
  (scene.fog as T.FogExp2).density = Math.min(0.0015, 0.8 / extent);
}
function fit() {
  if (!nodes.size) return;
  const box = worldBounds(),
    center = box.getCenter(new T.Vector3()),
    size = box.getSize(new T.Vector3());
  const distance = Math.max(size.x, size.y, size.z) * 0.8 + 20;
  controls.target.copy(center);
  camera.position
    .copy(center)
    .add(new T.Vector3(distance * 0.55, -distance * 0.8, distance * 0.85));
}
function visibleIds() {
  const term = $("search").value.trim().toLowerCase();
  return new Set(
    topology.nodes
      .filter(
        (n) =>
          !term || `${n.identity.pid} ${n.name}`.toLowerCase().includes(term),
      )
      .map((n) => key(n.identity)),
  );
}
function disposeGroup() {
  geometryGroup.traverse(disposeObject);
  scene.remove(geometryGroup);
  geometryGroup = new T.Group();
  scene.add(geometryGroup);
}
function rebuild(data: Topology, rearrange = false) {
  topology = data;
  const live = new Set(data.nodes.map((n) => key(n.identity)));
  for (const id of nodes.keys()) if (!live.has(id)) cpuGlows.delete(id);
  const liveEdges = new Set(data.edges.map((e) => e.id));
  for (const id of edgeStats.keys())
    if (!liveEdges.has(id)) edgeStats.delete(id);
  const layout = rearrange
    ? treeLayout(data.nodes)
    : stableLayout(
        data.nodes,
        new Map([...nodes].map(([id, n]) => [id, n.pos])),
      );
  nodes.clear();
  for (const n of data.nodes) {
    const id = key(n.identity),
      place = layout.get(id);
    nodes.set(id, {
      ...n,
      pos: new T.Vector3(place?.x ?? 0, place?.y ?? 0, 0),
      regions: layoutMaps(n.maps),
    });
  }
  recentFiles.prune(performance.now(), live);
  if (rearrange) filePositions.clear();
  network = networkGroups(data.edges);
  particles = particles.filter((p) => !p.networkId || network.has(p.networkId));
  networkPositions = networkLayout(
    network,
    new Map([...nodes].map(([id, n]) => [id, n.pos])),
    rearrange ? new Map() : networkPositions,
  );
  if (selectedNetwork && !network.has(selectedNetwork)) clearSelection();
  adaptWorld();
  if (
    (selected && !nodes.has(selected)) ||
    (selectedEdge && !liveEdges.has(selectedEdge))
  )
    clearSelection();
  buildScene();
  if (firstView && data.nodes.length) {
    fit();
    firstView = false;
  }
  if (selected || selectedEdge || selectedNetwork || selectedFile) details();
}
function buildScene() {
  disposeGroup();
  const visible = visibleIds();
  hullIds = [...visible];
  hull = new T.InstancedMesh(
    new T.BoxGeometry(2.2, 2.2, 8),
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
  const parentPos = [],
    parentColors = [];
  for (const childId of hullIds) {
    const child = nodes.get(childId)!,
      parentId = child.parent_id && key(child.parent_id),
      parent = parentId && nodes.get(parentId);
    if (!parent || !visible.has(parentId)) continue;
    parentViews.push({ parent: parentId, child: childId });
    parentPos.push(
      parent.pos.x,
      parent.pos.y,
      0.06,
      child.pos.x,
      child.pos.y,
      0.06,
    );
    parentColors.push(0.12, 0.28, 0.27, 0.12, 0.28, 0.27);
  }
  const parentGeometry = new T.BufferGeometry();
  parentGeometry.setAttribute(
    "position",
    new T.Float32BufferAttribute(parentPos, 3),
  );
  parentGeometry.setAttribute(
    "color",
    new T.Float32BufferAttribute(parentColors, 3),
  );
  parentLines = new T.LineSegments(
    parentGeometry,
    new T.LineBasicMaterial({
      vertexColors: true,
      transparent: true,
      opacity: 0.55,
      depthWrite: false,
    }),
  );
  parentLines.renderOrder = -1;
  geometryGroup.add(parentLines);
  const linePos: number[] = [],
    lineColors: number[] = [],
    lineEdges: (ConnectionPick | null)[] = [],
    layerList: { n: RenderNode; r: Region }[] = [];
  let regionLimit = 0;
  const addLine = (
    a: number[],
    b: number[],
    color: T.ColorRepresentation,
    edge: ConnectionPick | null = null,
  ) => {
    lineEdges.push(edge);
    linePos.push(...a, ...b);
    const c = new T.Color(color);
    lineColors.push(c.r, c.g, c.b, c.r, c.g, c.b);
  };
  for (let i = 0; i < hullIds.length; i++) {
    const n = nodes.get(hullIds[i])!,
      p = n.pos,
      userColors = processColors(n);
    dummy.position.set(p.x, p.y, 4);
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
      n.regions.length > 64 && hullIds[i] !== selected
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
    layers.setColorAt(i, new T.Color(regionColor(r)));
  }
  geometryGroup.add(layers);
  edgeViews = [];
  networkViews = [];
  const grouped = new Set(
    [...network.values()].flatMap((g) => g.members.map((e) => e.id)),
  );
  const visibleGroups = [...network.values()].filter(
    (g) => visible.has(key(g.a.process_id)) && networkPositions.has(g.id),
  );
  const markers = new T.InstancedMesh(
    new T.OctahedronGeometry(0.75),
    new T.MeshBasicMaterial({
      color: 0x68baff,
      transparent: true,
      opacity: 0.2,
    }),
    Math.max(1, visibleGroups.length),
  );
  markers.count = visibleGroups.length;
  markers.userData.network = visibleGroups;
  geometryGroup.add(markers);
  const markerEdges = new T.InstancedMesh(
    new T.OctahedronGeometry(0.75),
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
      pos = new T.Vector3(p.x, p.y, p.z),
      owner = nodes.get(key(group.a.process_id))!;
    dummy.position.copy(pos);
    dummy.scale.set(1, 1, 1);
    dummy.updateMatrix();
    markers.setMatrixAt(i, dummy.matrix);
    const start = owner.pos.clone().setZ(7.5),
      mid = start.clone().lerp(pos, 0.5);
    mid.z += 2;
    const curve = new T.QuadraticBezierCurve3(start, mid, pos);
    networkViews.push({ group, pos, curve });
    for (const e of group.members)
      edgeViews.push({ e, curve, networkId: group.id });
    const pts = curve.getPoints(24),
      pick = {
        ...group,
        b: null,
        shared: false,
        candidate: false,
        networkId: group.id,
      };
    for (let j = 0; j < 24; j++)
      addLine(pts[j].toArray(), pts[j + 1].toArray(), 0x68baff, pick);
  }
  const dashedPos = [],
    dashedEdges = [];
  for (const e of topology.edges) {
    if (grouped.has(e.id)) continue;
    const a = nodes.get(key(e.a.process_id)),
      b = e.b && nodes.get(key(e.b.process_id));
    if (
      !a ||
      !visible.has(key(a.identity)) ||
      (b && !visible.has(key(b.identity)))
    )
      continue;
    const start = a.pos
      .clone()
      .add(new T.Vector3(1.12, 0, 1 + (e.a.fd % 12) * 0.11));
    const end = b
      ? b.pos.clone().add(new T.Vector3(-1.12, 0, 1 + (e.b!.fd % 12) * 0.11))
      : start.clone().add(new T.Vector3(3.5, -2.5, -0.6));
    const mid = start.clone().lerp(end, 0.5);
    mid.z = 0.22;
    const curve = new T.QuadraticBezierCurve3(start, mid, end);
    edgeViews.push({ e, curve });
    const pts = curve.getPoints(16);
    for (let j = 0; j < 16; j++) {
      if (e.candidate || e.shared) {
        if (j % 2 === 0) {
          dashedPos.push(...pts[j], ...pts[j + 1]);
          dashedEdges.push(e);
        }
      } else
        addLine(
          pts[j].toArray(),
          pts[j + 1].toArray(),
          e.b ? 0x236857 : 0x24404c,
          e,
        );
    }
  }
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
  solidLines.userData.edges = lineEdges;
  geometryGroup.add(solidLines);
  const dashed = new T.BufferGeometry();
  dashed.setAttribute("position", new T.Float32BufferAttribute(dashedPos, 3));
  const dashedLines = new T.LineSegments(
    dashed,
    new T.LineBasicMaterial({
      color: 0x54828b,
      transparent: true,
      opacity: 0.3,
      depthWrite: false,
    }),
  );
  dashedLines.userData.edges = dashedEdges;
  geometryGroup.add(dashedLines);
  refreshFileScene();
  updateSelection();
}
function selectNetwork(id: string | null) {
  selectedFile = null;
  selected = null;
  selectedEdge = null;
  selectedNetwork = id;
  $("details").hidden = !id;
  updateSelection();
  if (id) details();
}
function networkDetails() {
  const group = network.get(selectedNetwork ?? "");
  if (!group) return;
  $("process-details").hidden = true;
  $("connection-details").hidden = false;
  $("connection-label").textContent = group.label;
  $("connection-state").textContent = "Network destination";
  const stats = group.members
    .map((e) => edgeStats.get(e.id))
    .filter((s): s is EdgeStat => s !== undefined);
  const latest = Math.max(0, ...stats.map((s) => s.time)),
    recent = stats.filter((s) => s.time === latest);
  $("connection-facts").textContent = recent.length
    ? `Latest window: ${recent.reduce((n, s) => n + s.bytes, 0)} bytes / ${recent.reduce((n, s) => n + s.count, 0)} operations · ${((performance.now() - latest) / 1000).toFixed(1)}s ago`
    : "Recent traffic —";
  const content = document.createDocumentFragment(),
    heading = document.createElement("p");
  heading.textContent = `${nodes.get(key(group.a.process_id))?.name || ""} · PID ${group.a.process_id.pid} → ${remoteLabel(group.socket)} (${group.socket.remote}) · ${group.members.length} connections`;
  content.append(heading);
  for (const e of group.members) {
    const row = document.createElement("div");
    row.className = "endpoint";
    const button = document.createElement("button");
    button.textContent = `FD ${e.a.fd}${e.a.fd_count > 1 ? ` (+${e.a.fd_count - 1} shared FDs)` : ""} · ${e.socket!.state}`;
    button.onclick = () => selectConnection(e.id);
    const address = document.createElement("span");
    address.textContent = `${e.socket!.local || "—"} → ${e.socket!.remote}`;
    const stat = edgeStats.get(e.id),
      observed = document.createElement("span");
    observed.textContent = stat
      ? `${stat.bytes} bytes / ${stat.count} operations · ${((performance.now() - stat.time) / 1000).toFixed(1)}s ago`
      : "Recent traffic —";
    row.append(button, address, observed);
    content.append(row);
  }
  $("connection-endpoints").replaceChildren(content);
}
function networkVisuals() {
  return networkViews.map((v) => ({
    id: v.group.id,
    label: v.group.label,
    position: v.pos.toArray(),
    members: v.group.members.map((e) => e.id),
    screen: v.pos.clone().project(camera).toArray(),
    pathScreen: v.curve.getPoint(0.5).project(camera).toArray(),
  }));
}
function networkParticles() {
  return particles
    .filter((p) => p.networkId)
    .map((p) => ({ id: p.networkId, direction: p.direction }));
}

function refreshFileScene() {
  fileGroup.traverse(disposeObject);
  scene.remove(fileGroup);
  fileGroup = new T.Group();
  scene.add(fileGroup);
  filePositions = fileLayout(
    recentFiles.entries,
    new Map([...nodes].map(([id, n]) => [id, n.pos])),
    filePositions,
  );
  const visible = visibleIds(),
    files = [...recentFiles.entries.values()].filter((f) =>
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
  geometry.setAttribute("position", new T.Float32BufferAttribute(vertices, 3));
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
  particles = particles.filter((p) => !p.fileId || fileViews.has(p.fileId));
  if (selectedFile && !recentFiles.entries.has(selectedFile)) clearSelection();
  adaptWorld();
  updateSelection();
}
function selectFile(id: string | null) {
  selected = null;
  selectedEdge = null;
  selectedNetwork = null;
  selectedFile = id;
  $("details").hidden = !id;
  updateSelection();
  if (id) details();
}
function fileDetails() {
  const f = recentFiles.entries.get(selectedFile ?? "");
  if (!f) return;
  $("process-details").hidden = true;
  $("connection-details").hidden = false;
  $("connection-label").textContent = f.label;
  $("connection-state").textContent = "Regular file";
  $("connection-facts").textContent =
    `Observed READ: ${f.readBytes} bytes / ${f.readCount} operations · WRITE: ${f.writeBytes} bytes / ${f.writeCount} operations`;
  const path = document.createElement("p");
  path.textContent = f.path || `Path unavailable · ${f.resource}`;
  const identity = document.createElement("p");
  identity.textContent = `${nodes.get(key(f.process_id))?.name || "Unknown process"} · PID ${f.process_id.pid} · ${f.resource}`;
  const link = document.createElement("a");
  link.href = `/process/${f.process_id.pid}`;
  link.textContent = "Open process details ↗";
  $("connection-endpoints").replaceChildren(path, identity, link);
}
function fileVisuals() {
  return [...fileViews.values()].map((v) => ({
    id: v.file.id,
    label: v.file.label,
    position: v.pos.toArray(),
    screen: v.pos.clone().project(camera).toArray(),
    pathScreen: v.curve.getPoint(0.5).project(camera).toArray(),
  }));
}
function fileParticles() {
  return particles
    .filter((p) => p.fileId)
    .map((p) => ({ id: p.fileId, direction: p.direction, color: p.color }));
}
function pruneFiles(now = performance.now()) {
  if (recentFiles.prune(now, new Set(nodes.keys()))) refreshFileScene();
}

function cpuText(id: string) {
  const state = cpuGlows.get(id);
  if (!state) return "Observed CPU —";
  if (performance.now() - state.last >= 500) return "Observed CPU idle";
  const cpus = state.cpus.length ? `CPU ${state.cpus.join(", ")}` : "Off CPU";
  return `${cpus} · ${state.running_threads} threads · ${(state.runtime_ns / 1e6).toFixed(2)} ms / ${state.window_ms} ms`;
}
function accessText(access: number) {
  return access === 0
    ? "READ"
    : access === 1
      ? "WRITE"
      : access === 2
        ? "READ / WRITE"
        : "UNKNOWN";
}
function endpoint(
  port: Port | null,
  title: string,
  socket: SocketEndpoint | null = null,
) {
  const div = document.createElement("div");
  div.className = "endpoint";
  const n = port && nodes.get(key(port.process_id));
  const heading = document.createElement("strong");
  heading.textContent = port
    ? n?.name || `Unknown process`
    : socket?.network_peer
      ? (remoteLabel(socket) ?? "Unknown destination")
      : "External / unknown";
  div.append(heading);
  if (!port) {
    const note = document.createElement("span");
    note.textContent = socket?.network_peer
      ? `${socket.protocol} · ${socket.state}`
      : "The peer process could not be identified.";
    div.append(note);
    return div;
  }
  const identity = document.createElement("span");
  identity.textContent = `${title} · PID ${port.process_id.pid} · ${n?.username ?? n?.uid ?? "unknown"}`;
  const fd = document.createElement("span");
  fd.textContent = `FD ${port.fd}${port.fd_count > 1 ? ` (+${port.fd_count - 1} shared FDs)` : ""} · ${accessText(port.access)}`;
  const resource = document.createElement("span");
  resource.textContent = port.resource;
  const link = document.createElement("a");
  link.href = `/process/${port.process_id.pid}`;
  link.textContent = "Open process details ↗";
  div.append(identity, fd, resource, link);
  return div;
}
function details() {
  $("connection-kind").textContent = selectedFile
    ? "SELECTED FILE"
    : "SELECTED CONNECTION";
  if (selectedFile) {
    fileDetails();
    return;
  }
  if (selectedNetwork) {
    networkDetails();
    return;
  }
  const process = $("process-details"),
    connection = $("connection-details");
  if (selected) {
    const n = nodes.get(selected ?? "");
    if (!n) return;
    process.hidden = false;
    connection.hidden = true;
    $("name").textContent = n.name;
    $("pid").replaceChildren(
      document.createTextNode(`PID ${n.identity.pid} · `),
    );
    for (const [role, uid, name, color] of [
      ["Real", n.uid, n.username, processColors(n).real],
      ["Effective", n.euid, n.effective_username, processColors(n).effective],
    ] as const) {
      const label = document.createElement("span");
      label.style.color = color;
      label.textContent = `${role}: ${name ?? uid ?? "unknown"}${name ? ` (${uid})` : ""} `;
      $("pid").append(label);
    }
    $("facts").textContent =
      `CPU ${(n.cpu_percent ?? 0).toFixed(1)}% · ${cpuText(selected)} · RSS ${(n.rss_bytes / 1048576).toFixed(1)} MiB`;
    $("inspect").href = `/process/${n.identity.pid}`;
    $("memory-status").textContent = `Memory: ${memoryStatus}`;
    return;
  }
  const e = topology.edges.find((edge) => edge.id === selectedEdge);
  if (!e) return;
  process.hidden = true;
  connection.hidden = false;
  $("connection-label").textContent = e.label;
  $("connection-state").textContent = connectionState(e);
  const stat = edgeStats.get(e.id);
  $("connection-facts").textContent = stat
    ? `Latest activity: ${stat.bytes} bytes / ${stat.count} operations · ${((performance.now() - stat.time) / 1000).toFixed(1)}s ago`
    : "Recent traffic —";
  $("connection-endpoints").replaceChildren(
    endpoint(e.a, "ENDPOINT A"),
    endpoint(e.b, "ENDPOINT B", e.socket),
  );
  if (e.socket) {
    const info = document.createElement("p");
    info.textContent = `${e.socket.protocol} ${e.socket!.state} · ${e.socket!.local || "—"} → ${e.socket.remote || "—"}`;
    $("connection-endpoints").append(info);
  }
  const group = [...network.values()].find((g) =>
    g.members.some((member) => member.id === e.id),
  );
  if (group) {
    const back = document.createElement("button");
    back.textContent = "Show all connections to this destination";
    back.onclick = () => selectNetwork(group.id);
    $("connection-endpoints").append(back);
  }
}
function updateParentSelection() {
  if (!parentLines?.geometry.attributes.color) return;
  const active = new Set();
  if (selected) {
    let child = selected;
    const seen = new Set();
    while (child && !seen.has(child)) {
      seen.add(child);
      const parentId = nodes.get(child)?.parent_id;
      const parent: string | null | undefined = parentId && key(parentId);
      if (!parent || !nodes.has(parent)) break;
      active.add(`${parent}>${child}`);
      child = parent;
    }
    for (const [id, n] of nodes) {
      const parent = n.parent_id && key(n.parent_id);
      if (parent === selected) active.add(`${selected}>${id}`);
    }
  }
  const colors = parentLines.geometry.attributes.color;
  for (let i = 0; i < parentViews.length; i++) {
    const view = parentViews[i],
      bright = active.has(`${view.parent}>${view.child}`);
    const color: [number, number, number] = bright
      ? [0.55, 1, 0.82]
      : [0.12, 0.28, 0.27];
    colors.setXYZ(i * 2, ...color);
    colors.setXYZ(i * 2 + 1, ...color);
  }
  colors.needsUpdate = true;
}
function updateSelection() {
  syncMemorySelection();
  const n = nodes.get(selected ?? "");
  selection.visible = !!n;
  if (n) selection.position.set(n.pos.x, n.pos.y, 4);
  const view = selectedFile
    ? fileViews.get(selectedFile)
    : selectedNetwork
      ? networkViews.find((v) => v.group.id === selectedNetwork)
      : edgeViews.find((v) => v.e.id === selectedEdge);
  edgeSelection.visible = !!view;
  if (view) edgeSelection.geometry.setFromPoints(view.curve.getPoints(32));
  updateParentSelection();
}
function clearSelection() {
  selectedFile = null;
  selectedNetwork = null;
  selected = null;
  selectedEdge = null;
  $("details").hidden = true;
  updateSelection();
}
function select(id: string | null, focus = false) {
  selectedFile = null;
  selectedNetwork = null;
  selected = id;
  selectedEdge = null;
  $("details").hidden = !id;
  if (id) {
    details();
    if (focus) {
      const p = nodes.get(id)!.pos;
      controls.target.copy(p).add(new T.Vector3(0, 0, 4));
      camera.position.copy(p).add(new T.Vector3(13, -20, 15));
    }
  }
  updateSelection();
}
function selectConnection(id: string | null) {
  selectedFile = null;
  selectedNetwork = null;
  selected = null;
  selectedEdge = id;
  $("details").hidden = !id;
  updateSelection();
  if (id) details();
}
function activity(data: SpaceActivity) {
  memoryStatus = data.status?.memory ?? memoryStatus;
  const now = performance.now();
  for (const pid of data.invalidated || []) {
    for (const n of nodes.values())
      if (n.identity.pid === pid) {
        n.invalidated = true;
      }
  }
  const fileChanged = recentFiles.ingest(
    data.files || [],
    now,
    new Set(nodes.keys()),
  );
  if (fileChanged) refreshFileScene();
  for (const e of data.files || []) {
    const v = fileViews.get(fileKey(e));
    if (!v || !(e.bytes > 0) || !(e.count > 0)) continue;
    const plan = ipcParticlePlan(e.count, reduced);
    for (const offset of plan.offsets)
      particles.push({
        start: now + offset,
        duration: plan.duration,
        curve: v.curve,
        direction: e.write ? 1 : -1,
        fileId: v.file.id,
        color: 0xffffff,
      });
  }

  for (const e of data.cpu || []) {
    const id = key(e.process_id);
    if (!nodes.has(id)) continue;
    cpuGlows.set(id, { ...e, window_ms: data.window_ms || 100, last: now });
  }
  for (const e of data.memory || []) {
    const n = nodes.get(key(e.process_id));
    if (
      !n ||
      key(e.process_id) !== selected ||
      n.invalidated ||
      n.maps_epoch !== e.maps_epoch
    )
      continue;
    const z = addressZ(n.regions, e.page);
    if (z === null) continue;
    particles.push({
      start: now,
      duration: reduced ? 150 : 800,
      pos: n.pos.clone().add(new T.Vector3(0, 0, z)),
      color:
        e.mode === "write" ? 0xffb86c : e.mode === "read" ? 0x65fff0 : 0xccccff,
      strength: Math.min(2, 0.4 + Math.log2(e.count + 1) * 0.3),
      memory: true,
    });
  }
  for (const e of data.ipc || []) {
    const links = edgeViews.filter((v) => edgeDirection(v.e, e) !== null);
    const exact = links.filter((v) => !v.e.candidate);
    const candidates = exact.length ? exact : links;
    // Multiple holders cannot identify the actual receiver. Only flash the actor's port in that case.
    const plan = ipcParticlePlan(e.count, reduced);
    if (candidates.length !== 1) {
      const n = nodes.get(key(e.process_id));
      if (n)
        particles.push({
          start: now,
          duration: plan.duration,
          pos: n.pos.clone().add(new T.Vector3(1.2, 0, 1)),
          color: 0xffffff,
        });
      continue;
    }
    const v = candidates[0],
      prior = edgeStats.get(v.e.id);
    edgeStats.set(v.e.id, {
      bytes: e.bytes + (prior?.time === now ? prior.bytes : 0),
      count: e.count + (prior?.time === now ? prior.count : 0),
      time: now,
    });
    for (const offset of plan.offsets)
      particles.push({
        start: now + offset,
        duration: plan.duration,
        curve: v.curve,
        direction: edgeDirection(v.e, e),
        networkId: v.networkId,
        color: 0xffffff,
      });
  }
  if (particles.length > CAP) particles = particles.slice(-CAP);
  if (selected || selectedEdge || selectedNetwork || selectedFile) details();
}
function setRay(event: MouseEvent) {
  mouse.set(
    (event.clientX / innerWidth) * 2 - 1,
    (-event.clientY / innerHeight) * 2 + 1,
  );
  ray.setFromCamera(mouse, camera);
}
function hit(event: MouseEvent) {
  setRay(event);
  return hull && ray.intersectObject(hull)[0];
}
function edgeHit(event: MouseEvent): PickResult | null {
  setRay(event);
  ray.params.Line.threshold = 0.25;
  for (const result of ray.intersectObjects([
    ...geometryGroup.children,
    ...fileGroup.children,
  ])) {
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
        b: null,
        shared: false,
        candidate: false,
        networkId: group.id,
      };
    const edge =
      result.index === undefined
        ? undefined
        : data.edges?.[Math.floor(result.index / 2)];
    if (edge) return edge;
  }
  return null;
}
canvas.addEventListener("pointerdown", (e) => {
  down = { x: e.clientX, y: e.clientY };
});
canvas.addEventListener("pointerup", (e) => {
  if (!down || Math.hypot(e.clientX - down.x, e.clientY - down.y) > 5) return;
  const h = hit(e);
  if (h) select(hullIds[h.instanceId!]);
  else {
    const edge = edgeHit(e);
    if (edge) {
      if ("fileId" in edge) selectFile(edge.fileId);
      else if (edge.networkId) selectNetwork(edge.networkId);
      else selectConnection(edge.id);
    } else clearSelection();
  }
});
canvas.addEventListener("dblclick", (e) => {
  const h = hit(e);
  if (h) select(hullIds[h.instanceId!], true);
});
canvas.addEventListener("pointermove", (e) => {
  hoveredNetwork = null;
  hoveredFile = null;
  const h = hit(e);
  let text = "";
  if (h) {
    const id = hullIds[h.instanceId!],
      n = nodes.get(id)!,
      r = n.regions.find((r) => h.point.z >= r.z && h.point.z <= r.z + r.h);
    text = `${n.name} / ${n.identity.pid}\n${cpuText(id)}${r ? `\n${r.permissions} ${r.pathname || "anonymous"}\n${r.start} → ${r.end}` : ""}`;
  } else {
    const edge = edgeHit(e);
    if (edge && "fileId" in edge) {
      hoveredFile = edge.fileId;
      const f = recentFiles.entries.get(edge.fileId)!;
      text = `${f.path || f.resource}\nPID ${f.process_id.pid} · READ ${f.readBytes} bytes · WRITE ${f.writeBytes} bytes`;
    } else if (edge) {
      hoveredNetwork = edge.networkId || null;
      const stat = edgeStats.get(edge.id);
      text = `${edge.label}${edge.candidate ? " · Candidate peer" : ""}${edge.shared ? " · Shared FD" : ""}\nPID ${edge.a.process_id.pid} / FD ${edge.a.fd}${edge.a.fd_count > 1 ? ` (+${edge.a.fd_count - 1} shared FDs)` : ""} ↔ ${edge.b ? `PID ${edge.b.process_id.pid} / FD ${edge.b.fd}` : edge.socket?.network_peer ? remoteLabel(edge.socket) : connectionState(edge)}${stat ? `\nLatest window: ${stat.bytes} bytes / ${stat.count} operations (${((performance.now() - stat.time) / 1000).toFixed(1)}s ago)` : ""}`;
    }
  }
  $("hover").hidden = !text;
  if (text) {
    $("hover").textContent = text;
    $("hover").style.left =
      `${Math.max(0, Math.min(e.clientX + 15, innerWidth - 350))}px`;
    $("hover").style.top = `${e.clientY + 15}px`;
  }
});

$("rearrange").onclick = () => {
  particles = [];
  rebuild(topology, true);
  fit();
};
$("close").onclick = () => select(null);
$("reset").onclick = () => {
  fit();
  $("search").value = "";
  select(null);
  buildScene();
};
$("search").oninput = () => {
  particles = [];
  buildScene();
};
$("search").onkeydown = (e) => {
  if (e.key === "Enter") {
    const ids = visibleIds();
    if (ids.size) select([...ids][0], true);
  }
};
addEventListener("resize", () => {
  renderer.setSize(innerWidth, innerHeight);
  resizeLabels();
  camera.aspect = innerWidth / innerHeight;
  camera.updateProjectionMatrix();
});
function animate(now: number) {
  requestAnimationFrame(animate);
  if (document.hidden) return;
  controls.update();
  if (now - lastFilePrune >= 1000) {
    lastFilePrune = now;
    if (recentFiles.prune(now, new Set(nodes.keys()))) refreshFileScene();
  }
  particles = particles.filter((p) => now - p.start < p.duration);
  let i = 0;
  for (const p of particles) {
    if (now < p.start) continue;
    const t = (now - p.start) / p.duration,
      point = p.curve ? p.curve.getPoint(p.direction === 1 ? t : 1 - t) : p.pos;
    positions.set(point.toArray(), i * 3);
    const c = new T.Color(p.color).multiplyScalar(
      (p.strength || 1) * (1 - t * 0.8),
    );
    colors.set([c.r, c.g, c.b], i * 3);
    i++;
  }
  pg.setDrawRange(0, i);
  pg.attributes.position.needsUpdate = true;
  pg.attributes.color.needsUpdate = true;
  if (baseGlow && haloGlow) {
    for (let j = 0; j < hullIds.length; j++) {
      const level = cpuGlowLevel(cpuGlows.get(hullIds[j]), now);
      baseGlow.setColorAt(
        j,
        new T.Color().setRGB(0.45 * level, 1.15 * level, 1.05 * level),
      );
      haloGlow.setColorAt(
        j,
        new T.Color().setRGB(0.18 * level, 0.85 * level, 0.72 * level),
      );
    }
    if (baseGlow.instanceColor) baseGlow.instanceColor.needsUpdate = true;
    if (haloGlow.instanceColor) haloGlow.instanceColor.needsUpdate = true;
  }
  renderer.render(scene, camera);
  drawLabels();
  frames++;
  if (now - frameTime > 1000) {
    const fps = Math.round((frames * 1000) / (now - frameTime));
    slowFrames = fps < 24 ? slowFrames + 1 : 0;
    if (slowFrames >= 2 && renderScale > 0.55) {
      renderScale = Math.max(0.5, renderScale * 0.75);
      renderer.setPixelRatio(renderScale);
      slowFrames = 0;
    }
    $("fps").textContent =
      `${fps} FPS · ${Math.round(renderScale * 100)}% render`;
    frames = 0;
    frameTime = now;
  }
}
requestAnimationFrame(animate);
function cpuGlowVisual(id: string) {
  const index = hullIds.indexOf(id);
  if (index < 0 || !baseGlow) return null;
  const color = new T.Color();
  baseGlow.getColorAt(index, color);
  return { r: color.r, g: color.g, b: color.b };
}
function cameraView() {
  return {
    position: camera.position.toArray(),
    target: controls.target.toArray(),
  };
}
function processPosition(id: string) {
  const pos = nodes.get(id)?.pos;
  return pos && { x: pos.x, y: pos.y, z: pos.z };
}
function parentLineVisual(parent: string, child: string) {
  const index = parentViews.findIndex(
    (view) => view.parent === parent && view.child === child,
  );
  if (index < 0 || !parentLines) return null;
  const color = parentLines.geometry.attributes.color;
  return {
    r: color.getX(index * 2),
    g: color.getY(index * 2),
    b: color.getZ(index * 2),
  };
}
let token: string | null = null,
  source: EventSource | null = null,
  epoch = 0,
  heartbeat: number | undefined,
  retry: number | undefined,
  memoryStatus = "idle",
  renewQueue = Promise.resolve(),
  lastMemorySelection: string | null = null;
function selectionIdentity() {
  return nodes.get(selected ?? "")?.identity ?? null;
}
function syncMemorySelection() {
  const next = selected;
  if (next === lastMemorySelection) return;
  lastMemorySelection = next;
  particles = particles.filter((p) => !p.memory);
  memoryStatus = next ? "starting" : "idle";
  if (token) renew();
}
async function api(path: string, options: RequestInit): Promise<Lease> {
  const r = await fetch(path, options);
  const data: unknown = await r.json();
  if (!r.ok)
    throw new Error(
      data && typeof data === "object" && "error" in data
        ? String(data.error)
        : r.statusText,
    );
  return data as Lease;
}
async function start() {
  if (document.hidden) return;
  const e = ++epoch;
  clearTimeout(retry);
  try {
    const lease = await api("/api/space/leases", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        density: Number($("density").value),
        selected_process: selectionIdentity(),
      }),
    });
    if (e !== epoch || document.hidden) {
      release(lease.token);
      return;
    }
    token = lease.token;
    renew();
    source = new EventSource(
      `/api/space/events?token=${encodeURIComponent(token)}`,
    );
    source.onopen = () => {
      $("failure").hidden = true;
    };
    source.addEventListener("topology", (event) =>
      rebuild(JSON.parse(event.data)),
    );
    source.addEventListener("activity", (event) =>
      activity(JSON.parse(event.data)),
    );
    source.addEventListener("metrics", (event) => {
      for (const m of JSON.parse(event.data) as Pick<
        ProcessSummary,
        "identity" | "cpu_percent" | "rss_bytes"
      >[]) {
        const n = nodes.get(key(m.identity));
        if (n) Object.assign(n, m);
      }
      if (selected) details();
    });
    source.addEventListener("gap", () => {
      particles = [];
    });
    heartbeat = setInterval(renew, 10000);
  } catch (err) {
    $("failure").textContent = err instanceof Error ? err.message : String(err);
    $("failure").hidden = false;
    retry = setTimeout(start, 3000);
  }
}
function release(t: string) {
  fetch("/api/space/leases", {
    method: "DELETE",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ token: t }),
    keepalive: true,
  }).catch(() => {});
}
function stop() {
  epoch++;
  clearTimeout(retry);
  clearInterval(heartbeat);
  source?.close();
  source = null;
  if (token) release(token);
  token = null;
  particles = [];
  cpuGlows.clear();
  recentFiles.clear();
  refreshFileScene();
}
function renew() {
  const t = token;
  if (!t) return Promise.resolve();
  const body = JSON.stringify({
    token: t,
    density: Number($("density").value),
    selected_process: selectionIdentity(),
  });
  renewQueue = renewQueue.then(async () => {
    if (t !== token) return;
    try {
      await api("/api/space/leases", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body,
      });
    } catch (e) {
      if (t === token) {
        stop();
        start();
      }
    }
  });
  return renewQueue;
}

$("density").onchange = renew;
document.addEventListener("visibilitychange", () => {
  if (document.hidden) stop();
  else start();
});
addEventListener("pagehide", stop);
addEventListener("pageshow", (e) => {
  if (e.persisted) start();
});
start();

export {
  selectFile,
  fileVisuals,
  fileParticles,
  pruneFiles,
  rebuild as renderTopology,
  activity as renderActivity,
  fit as fitTopology,
  select as selectProcess,
  selectConnection,
  selectNetwork,
  networkVisuals,
  networkParticles,
  cameraView,
  processPosition,
  parentLineVisual,
  cpuGlows as cpuGlowStates,
  cpuGlowVisual,
};
