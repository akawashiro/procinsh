// Animate activity, draw the scene and labels, and adapt rendering resolution.
import { spaceNow } from "./clock.js";
import type * as T from "three";
import { key, signalName } from "./model.js";
import { emptyActivityRetention } from "./types.js";
import type {
  ActivityUpdate,
  ConnectionCurve,
  CpuGlow,
  SelectionState,
  HoverState,
} from "./types.js";
import type { RenderView } from "./contracts.js";
// Each sample covers roughly one second of visible rendering. Separate thresholds
// and consecutive windows keep transient scene rebuilds from changing resolution.
class AdaptiveRenderScale {
  readonly max: number;
  readonly min: number;
  scale: number;
  private slowWindows = 0;
  private healthyWindows = 0;

  constructor(devicePixelRatio: number) {
    this.max = Math.min(devicePixelRatio, 1.5);
    this.min = Math.min(this.max, 0.5);
    this.scale = this.max;
  }

  resetSampling() {
    this.slowWindows = 0;
    this.healthyWindows = 0;
  }

  sample(fps: number): number {
    this.slowWindows = fps < 24 ? this.slowWindows + 1 : 0;
    this.healthyWindows = fps >= 45 ? this.healthyWindows + 1 : 0;
    if (this.slowWindows >= 3) {
      this.scale = Math.max(this.min, this.scale * 0.9);
      this.resetSampling();
    } else if (this.healthyWindows >= 5) {
      this.scale = Math.min(this.max, this.scale / 0.9);
      this.resetSampling();
    }
    return this.scale;
  }
}

const IPC_PARTICLE_DURATION_MS = 2000;
const IPC_PARTICLE_STAGGER_MS = 100;
const REDUCED_MOTION_PARTICLE_DURATION_MS = 150;

function ipcParticlePlan(operationCount: number, reducedMotion = false) {
  if (reducedMotion)
    return { duration: REDUCED_MOTION_PARTICLE_DURATION_MS, offsets: [0] };
  const count = Number.isFinite(operationCount)
    ? Math.max(1, operationCount)
    : 1;
  const particles = Math.min(
    6,
    Math.max(2, 1 + Math.ceil(Math.log2(count + 1))),
  );
  return {
    duration: IPC_PARTICLE_DURATION_MS,
    offsets: Array.from(
      { length: particles },
      (_, index) => index * IPC_PARTICLE_STAGGER_MS,
    ),
  };
}

function cpuGlowLevel(
  state:
    | Pick<CpuGlow, "last" | "window_ms" | "runtime_ns" | "running_threads">
    | undefined,
  now: number,
  afterglowMs = 500,
) {
  if (!state || now < state.last || now - state.last >= afterglowMs) return 0;
  const windowNs = Math.max(1, state.window_ms * 1e6);
  const utilization = Math.min(1, state.runtime_ns / windowNs);
  const activity = Math.min(1, 0.2 + Math.sqrt(utilization) * 0.8);
  const peak =
    state.running_threads > 0
      ? Math.min(1, 0.82 + Math.log2(state.running_threads + 1) * 0.12)
      : activity;
  return peak * (1 - (now - state.last) / afterglowMs);
}

// Labels fade between shown and hidden so overlap culling never flickers.
const LABEL_FADE_MS = 150;

type LabelState = { alpha: number; shown: boolean };
type LabelCandidate = {
  x: number;
  y: number;
  width: number;
  depth: number;
  active: boolean;
  state: LabelState;
};

/**
 * Hide inactive labels that overlap a nearer one and move each label's alpha
 * toward the result by `fadeStep`. Labels shown last frame keep priority so
 * near-equal depths do not swap. Returns the labels to draw, in draw order.
 */
function placeLabels<L extends LabelCandidate>(
  labels: L[],
  fadeStep: number,
): L[] {
  labels.sort(
    (a, b) =>
      Number(b.active) - Number(a.active) ||
      Number(b.state.shown) - Number(a.state.shown) ||
      a.depth - b.depth,
  );
  const occupied: {
    left: number;
    right: number;
    top: number;
    bottom: number;
  }[] = [];
  return labels.filter((label) => {
    const box = {
      left: label.x - label.width / 2,
      right: label.x + label.width / 2,
      top: label.y - 12,
      bottom: label.y + 2,
    };
    const shown =
      label.active ||
      !occupied.some(
        (o) =>
          box.left < o.right &&
          box.right > o.left &&
          box.top < o.bottom &&
          box.bottom > o.top,
      );
    if (shown) occupied.push(box);
    const { state } = label;
    state.shown = shown;
    state.alpha = shown
      ? Math.min(1, state.alpha + fadeStep)
      : Math.max(0, state.alpha - fadeStep);
    return state.alpha > 0;
  });
}

const SIGNAL_CAP = 256;
const SIGNAL_DURATION_MS = 1700;
const SIGNAL_PULSE_MS = 400;
type SignalFlight = {
  source: string;
  destination: string;
  label: string;
  start: number;
  duration: number;
  curve: T.QuadraticBezierCurve3;
  arrived: boolean;
};

type Particle = {
  processIds: string[];
  start: number;
  duration: number;
  color: number;
  networkId?: string;
  connectionId?: string;
  socketOwnerKey?: string;
  fileId?: string;
  netlinkId?: string;
} & (
  | { curve: ConnectionCurve; direction: number | null; pos?: never }
  | { pos: T.Vector3; curve?: never; direction?: never }
);
function context2d(canvas: HTMLCanvasElement): CanvasRenderingContext2D {
  const context = canvas.getContext("2d");
  if (!context) throw new Error("Canvas 2D is unavailable");
  return context;
}

export interface SpaceRendererOptions {
  graphics: typeof T;
  canvas: HTMLCanvasElement;
  labelCanvas: HTMLCanvasElement;
  fpsLabel: HTMLElement;
  failure: HTMLElement;
  scene: T.Scene;
  camera: T.Camera;
  view(): RenderView;
  selection(): SelectionState & HoverState;
  cpuGlows: ReadonlyMap<string, CpuGlow>;
  beforeFrame(now: number): void;
}

export function createSpaceRenderer({
  graphics,
  canvas,
  labelCanvas,
  fpsLabel,
  failure,
  scene,
  camera,
  view: readView,
  selection: readSelection,
  cpuGlows,
  beforeFrame,
}: SpaceRendererOptions) {
  const labelContext = context2d(labelCanvas);
  const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const renderResolution = new AdaptiveRenderScale(devicePixelRatio);
  let particles: Particle[] = [],
    frames = 0,
    frameTime = performance.now(),
    running = false,
    frame: number | undefined;
  let signalFlights: SignalFlight[] = [];
  const signalPulses = new Map<string, number>();
  const labelPoint = new graphics.Vector3();
  const labelStates = new Map<string, LabelState>();
  let labelTime: number | undefined;
  let renderer: T.WebGLRenderer;
  try {
    renderer = new graphics.WebGLRenderer({
      canvas,
      antialias: false,
      alpha: false,
    });
  } catch (e) {
    failure.hidden = false;
    failure.textContent =
      "WebGL2 is unavailable. Use Go to list view to return to the process list.";
    throw e;
  }
  renderer.setPixelRatio(renderResolution.scale);
  renderer.setSize(innerWidth, innerHeight);
  renderer.setClearColor(0x03090e);
  renderer.outputColorSpace = graphics.SRGBColorSpace;
  const CAP = 8192,
    positions = new Float32Array(CAP * 3),
    colors = new Float32Array(CAP * 3);
  const pg = new graphics.BufferGeometry();
  pg.setAttribute("position", new graphics.BufferAttribute(positions, 3));
  pg.setAttribute("color", new graphics.BufferAttribute(colors, 3));
  pg.setDrawRange(0, 0);
  const particleCanvas = document.createElement("canvas");
  particleCanvas.width = particleCanvas.height = 64;
  const particleContext = context2d(particleCanvas);
  particleContext.fillStyle = "#fff";
  particleContext.beginPath();
  particleContext.arc(32, 32, 30, 0, Math.PI * 2);
  particleContext.fill();
  const particleTexture = new graphics.CanvasTexture(particleCanvas);
  const points = new graphics.Points(
    pg,
    new graphics.PointsMaterial({
      map: particleTexture,
      alphaTest: 0.1,
      size: 0.4,
      vertexColors: true,
      transparent: true,
      opacity: 0.95,
      blending: graphics.AdditiveBlending,
      depthWrite: false,
      sizeAttenuation: true,
    }),
  );
  points.frustumCulled = false;
  const signalGeometry = new graphics.BufferGeometry();
  signalGeometry.setAttribute("position", pg.attributes.position);
  signalGeometry.setAttribute("color", pg.attributes.color);
  signalGeometry.setDrawRange(0, 0);
  const signalMaterial = points.material.clone();
  signalMaterial.size = 0.8;
  const signalPoints = new graphics.Points(signalGeometry, signalMaterial);
  signalPoints.frustumCulled = false;
  scene.add(points, signalPoints);
  function resizeLabels() {
    const ratio = Math.min(devicePixelRatio, 2);
    labelCanvas.width = Math.round(innerWidth * ratio);
    labelCanvas.height = Math.round(innerHeight * ratio);
    labelContext.setTransform(ratio, 0, 0, ratio, 0, 0);
  }
  function drawLabels(now: number, wallTime: number) {
    const fadeStep =
      labelTime === undefined || reduced
        ? 1
        : Math.min(1, Math.max(0, wallTime - labelTime) / LABEL_FADE_MS);
    labelTime = wallTime;
    const { nodes, hullIds, networkViews, fileViews, netlinkViews } =
      readView();
    const {
      process: selected,
      connection: selectedEdge,
      network: selectedNetwork,
      file: selectedFile,
      netlink: selectedNetlink,
      hoveredNetwork,
      hoveredFile,
      hoveredNetlink,
    } = readSelection();
    labelContext.clearRect(0, 0, innerWidth, innerHeight);
    labelContext.font =
      "10px ui-monospace, SFMono-Regular, Consolas, monospace";
    labelContext.textAlign = "center";
    labelContext.textBaseline = "bottom";
    labelContext.lineJoin = "round";
    labelContext.lineWidth = 3;
    const labels: (LabelCandidate & {
      name: string;
      max: number;
      external: boolean;
    })[] = [];
    const seen = new Set<string>();
    const add = (
      key: string,
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
      let state = labelStates.get(key);
      if (!state) {
        state = { alpha: 0, shown: false };
        labelStates.set(key, state);
      }
      seen.add(key);
      const max = external ? 300 : 130;
      labels.push({
        name,
        max,
        x,
        y,
        width: Math.min(max, labelContext.measureText(name).width) + 6,
        depth: labelPoint.z,
        active,
        external,
        state,
      });
    };
    for (const id of hullIds) {
      const n = nodes.get(id);
      if (n)
        add(`process:${id}`, n.name, n.pos.clone().setZ(8.55), id === selected);
    }
    for (const v of networkViews)
      add(
        `network:${v.group.id}`,
        v.group.label,
        v.pos.clone().add(new graphics.Vector3(0, 0, 1)),
        v.group.id === selectedNetwork ||
          v.group.id === hoveredNetwork ||
          v.group.members.some((e) => e.id === selectedEdge),
        true,
      );
    for (const v of fileViews.values())
      add(
        `file:${v.file.id}`,
        v.file.label,
        v.pos.clone().add(new graphics.Vector3(0, 0, 0.7)),
        v.file.id === selectedFile || v.file.id === hoveredFile,
        true,
      );
    for (const v of netlinkViews.values())
      add(
        `netlink:${v.socket.id}`,
        v.socket.label,
        v.pos.clone().add(new graphics.Vector3(0, 0, 0.7)),
        v.socket.id === selectedNetlink || v.socket.id === hoveredNetlink,
        true,
      );
    for (const flight of signalFlights.slice(-12)) {
      const progress = Math.min(
        1,
        Math.max(0, (now - flight.start) / flight.duration),
      );
      add(
        `signal:${flight.start}:${flight.source}:${flight.destination}:${flight.label}`,
        flight.label,
        flight.curve.getPoint(progress).add(new graphics.Vector3(0, 0, 0.6)),
        true,
      );
    }
    for (const key of labelStates.keys())
      if (!seen.has(key)) labelStates.delete(key);
    for (const label of placeLabels(labels, fadeStep)) {
      const { name, x, y, max } = label;
      labelContext.globalAlpha = label.state.alpha;
      labelContext.strokeStyle = "rgba(2,9,14,.92)";
      labelContext.strokeText(name, x, y, max);
      labelContext.fillStyle = label.active
        ? "#e0faff"
        : label.external
          ? "#84caff"
          : "rgba(190,246,232,.88)";
      labelContext.fillText(name, x, y, max);
    }
    labelContext.globalAlpha = 1;
  }
  resizeLabels();

  function activity(update: ActivityUpdate) {
    const now = update.now,
      view = readView();
    for (const route of update.routes) {
      const plan = ipcParticlePlan(route.count, reduced);
      if (route.kind === "port") {
        const n = view.nodes.get(route.id);
        if (n)
          particles.push({
            start: now,
            duration: plan.duration,
            processIds: [route.id],
            pos: n.pos.clone().add(new graphics.Vector3(1.2, 0, 1)),
            socketOwnerKey: route.socketOwnerKey,
            color: 0xffffff,
          });
        continue;
      }
      const path =
        route.kind === "file"
          ? view.fileViews.get(route.id)
          : route.kind === "netlink"
            ? view.netlinkViews.get(route.id)
            : view.edgeViews.find((v) => v.e.id === route.id);
      if (!path) continue;
      const networkId =
        route.kind === "connection" && "networkId" in path
          ? path.networkId
          : undefined;
      const processIds =
        "file" in path
          ? [key(path.file.process_id)]
          : "socket" in path
            ? [key(path.socket.endpoint.process_id)]
            : [path.e.endpoint, path.e.peer]
                .filter((endpoint) => endpoint !== null)
                .map((endpoint) => key(endpoint.process_id));
      for (const offset of plan.offsets)
        particles.push({
          processIds,
          start: now + offset,
          duration: plan.duration,
          curve: path.curve,
          connectionId: route.kind === "connection" ? route.id : undefined,
          socketOwnerKey: route.socketOwnerKey,
          direction: route.direction ?? null,
          fileId: route.kind === "file" ? route.id : undefined,
          netlinkId: route.kind === "netlink" ? route.id : undefined,
          networkId,
          color: 0xffffff,
        });
    }
    for (const event of (update.signals || []).slice(-SIGNAL_CAP)) {
      const source = key(event.source_id),
        destination = key(event.destination_id);
      const src = view.nodes.get(source),
        dst = view.nodes.get(destination);
      if (
        !src ||
        !dst ||
        !view.hullIds.includes(source) ||
        !view.hullIds.includes(destination)
      )
        continue;
      const start = src.pos.clone().setZ(6),
        end = dst.pos.clone().setZ(6);
      const mid = start.clone().lerp(end, 0.5);
      mid.z += Math.min(8, 2 + start.distanceTo(end) * 0.12);
      if (source === destination) mid.x += 3;
      signalFlights.push({
        source,
        destination,
        label: signalName(event.signal),
        start: now,
        duration: reduced ? 300 : SIGNAL_DURATION_MS,
        curve: new graphics.QuadraticBezierCurve3(start, mid, end),
        arrived: false,
      });
    }
    signalFlights = signalFlights.slice(-SIGNAL_CAP);
    if (particles.length > CAP) particles = particles.slice(-CAP);
  }
  function activeResources(
    now = spaceNow(),
    suppressed: ReadonlySet<string> = new Set(),
  ) {
    const retained = emptyActivityRetention();
    for (const flight of signalFlights)
      if (now - flight.start < flight.duration + SIGNAL_PULSE_MS) {
        retained.processes.add(flight.source);
        retained.processes.add(flight.destination);
      }
    for (const [id, arrival] of signalPulses)
      if (now - arrival < SIGNAL_PULSE_MS) retained.processes.add(id);
    for (const particle of particles) {
      if (
        now - particle.start >= particle.duration ||
        (particle.socketOwnerKey && suppressed.has(particle.socketOwnerKey))
      )
        continue;
      for (const id of particle.processIds) retained.processes.add(id);
      if (particle.connectionId)
        retained.connections.add(particle.connectionId);
      if (particle.networkId) retained.networks.add(particle.networkId);
      if (particle.fileId) retained.files.add(particle.fileId);
      if (particle.netlinkId) retained.netlink.add(particle.netlinkId);
    }
    return retained;
  }
  function retainParticles(
    networkIds: ReadonlySet<string>,
    suppressed: ReadonlySet<string> = new Set(),
  ) {
    const view = readView();
    const visible = new Set(view.hullIds);
    const connections = new Set(view.edgeViews.map((v) => v.e.id));
    signalFlights = signalFlights.filter(
      (f) => visible.has(f.source) && visible.has(f.destination),
    );
    for (const id of signalPulses.keys())
      if (!visible.has(id)) signalPulses.delete(id);
    particles = particles.filter(
      (p) =>
        p.processIds.every((id) => !view.nodes.has(id) || visible.has(id)) &&
        (!p.connectionId || connections.has(p.connectionId)) &&
        (!p.socketOwnerKey || !suppressed.has(p.socketOwnerKey)) &&
        (!p.fileId || view.fileViews.has(p.fileId)) &&
        (!p.netlinkId || view.netlinkViews.has(p.netlinkId)) &&
        (!p.networkId || networkIds.has(p.networkId)),
    );
  }
  function animate(wallTime: number) {
    if (!running) return;
    frame = requestAnimationFrame(animate);
    if (document.hidden) return;
    beforeFrame(spaceNow(wallTime));
    const now = spaceNow(wallTime);
    particles = particles.filter((p) => now - p.start < p.duration);
    signalFlights = signalFlights.filter(
      (f) => now - f.start < f.duration + SIGNAL_PULSE_MS,
    );
    for (const [id, arrival] of signalPulses)
      if (now - arrival >= SIGNAL_PULSE_MS) signalPulses.delete(id);
    let i = 0;
    for (const f of signalFlights) {
      const elapsed = now - f.start;
      if (elapsed < 0) continue;
      if (elapsed >= f.duration) {
        if (!f.arrived) {
          f.arrived = true;
          if (
            !signalPulses.has(f.destination) &&
            signalPulses.size >= SIGNAL_CAP
          )
            signalPulses.delete(signalPulses.keys().next().value!);
          signalPulses.set(f.destination, f.start + f.duration);
        }
        continue;
      }
      const progress = elapsed / f.duration;
      for (let tail = 0; tail < (reduced ? 1 : 7); tail++) {
        const point = f.curve.getPoint(Math.max(0, progress - tail * 0.025));
        positions.set(point.toArray(), i * 3);
        const strength = tail === 0 ? 1 : 0.45 * (1 - tail / 7);
        colors.set([strength, 0.42 * strength, 0.95 * strength], i * 3);
        i++;
      }
    }
    const signalCount = i;
    for (const p of particles) {
      if (i >= CAP) break;
      if (now < p.start) continue;
      const t = (now - p.start) / p.duration,
        point = p.curve
          ? p.curve.getPoint(p.direction === 1 ? t : 1 - t)
          : p.pos;
      positions.set(point.toArray(), i * 3);
      const c = new graphics.Color(p.color).multiplyScalar(1 - t * 0.8);
      colors.set([c.r, c.g, c.b], i * 3);
      i++;
    }
    signalGeometry.setDrawRange(0, signalCount);
    pg.setDrawRange(signalCount, i - signalCount);
    pg.attributes.position.needsUpdate = true;
    pg.attributes.color.needsUpdate = true;
    const { baseGlow, haloGlow, hullIds } = readView();
    if (baseGlow && haloGlow) {
      for (let j = 0; j < hullIds.length; j++) {
        const level = cpuGlowLevel(cpuGlows.get(hullIds[j]), now);
        const arrival = signalPulses.get(hullIds[j]);
        const pulse =
          arrival === undefined
            ? 0
            : Math.max(0, 1 - (now - arrival) / SIGNAL_PULSE_MS);
        baseGlow.setColorAt(
          j,
          new graphics.Color().setRGB(
            0.45 * level + pulse,
            1.15 * level + pulse * 0.35,
            1.05 * level + pulse * 0.9,
          ),
        );
        haloGlow.setColorAt(
          j,
          new graphics.Color().setRGB(
            0.18 * level + pulse * 0.7,
            0.85 * level + pulse * 0.2,
            0.72 * level + pulse * 0.8,
          ),
        );
      }
      if (baseGlow.instanceColor) baseGlow.instanceColor.needsUpdate = true;
      if (haloGlow.instanceColor) haloGlow.instanceColor.needsUpdate = true;
    }
    renderer.render(scene, camera);
    drawLabels(now, wallTime);
    frames++;
    if (wallTime - frameTime > 1000) {
      const fps = Math.round((frames * 1000) / (wallTime - frameTime));
      const previousScale = renderResolution.scale;
      const renderScale = renderResolution.sample(fps);
      if (renderScale !== previousScale) renderer.setPixelRatio(renderScale);
      fpsLabel.textContent = `${fps} FPS · ${Math.round(renderScale * 100)}% render`;
      frames = 0;
      frameTime = wallTime;
    }
  }
  function networkVisuals() {
    return readView().networkViews.map((v) => ({
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

  function fileVisuals() {
    return [...readView().fileViews.values()].map((v) => ({
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
  function netlinkVisuals() {
    return [...readView().netlinkViews.values()].map((v) => ({
      id: v.socket.id,
      label: v.socket.label,
      position: v.pos.toArray(),
      screen: v.pos.clone().project(camera).toArray(),
      pathScreen: v.curve.getPoint(0.5).project(camera).toArray(),
    }));
  }
  function netlinkParticles() {
    return particles
      .filter((p) => p.netlinkId)
      .map((p) => ({ id: p.netlinkId, direction: p.direction }));
  }
  function resetRenderSampling() {
    frames = 0;
    frameTime = performance.now();
    renderResolution.resetSampling();
  }

  return {
    activity,
    activeResources,
    signalVisuals() {
      return signalFlights.map((f) => ({
        source: f.source,
        destination: f.destination,
        label: f.label,
        start: f.start,
        duration: f.duration,
        arrived: f.arrived,
        startPosition: f.curve.v0.toArray(),
        endPosition: f.curve.v2.toArray(),
      }));
    },
    retainParticles,
    networkVisuals,
    networkParticles,
    fileVisuals,
    fileParticles,
    netlinkVisuals,
    netlinkParticles,
    resetSampling: resetRenderSampling,
    clearParticles() {
      labelStates.clear();
      labelTime = undefined;
      particles = [];
      signalFlights = [];
      signalPulses.clear();
    },
    resize() {
      renderer.setSize(innerWidth, innerHeight);
      resizeLabels();
    },
    pause() {
      running = false;
      if (frame !== undefined) cancelAnimationFrame(frame);
      frame = undefined;
      resetRenderSampling();
    },
    resume() {
      if (running) return;
      running = true;
      resetRenderSampling();
      frame = requestAnimationFrame(animate);
    },
    dispose() {
      running = false;
      if (frame !== undefined) cancelAnimationFrame(frame);
      pg.dispose();
      points.material.dispose();
      signalGeometry.dispose();
      signalMaterial.dispose();
      particleTexture.dispose();
      scene.remove(points, signalPoints);
      renderer.dispose();
    },
  };
}
export type SpaceRenderer = ReturnType<typeof createSpaceRenderer>;

if (import.meta.vitest) {
  const { test } = import.meta.vitest;
  test("adaptive render scale", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
    {
      const resolution = new AdaptiveRenderScale(1);
      const sample = (fps: number, count: number) => {
        for (let i = 0; i < count; i++) resolution.sample(fps);
      };
      for (let i = 0; i < 20; i++) {
        sample(15, 2);
        sample(60, 1);
      }
      assert.equal(
        resolution.scale,
        1,
        "transient low FPS never accumulates into a permanent quality drop",
      );
      sample(15, 3);
      assert.equal(
        resolution.scale,
        0.9,
        "sustained low FPS reduces resolution gently",
      );
      sample(24, 10);
      assert.equal(
        resolution.scale,
        0.9,
        "24 FPS is outside the low-FPS range",
      );
      sample(60, 4);
      assert.equal(
        resolution.scale,
        0.9,
        "recovery waits for sustained healthy FPS",
      );
      sample(45, 1);
      assert.equal(
        resolution.scale,
        1,
        "healthy FPS restores initial sharpness without reloading",
      );
      sample(15, 100);
      assert.equal(
        resolution.scale,
        0.5,
        "sustained load respects the lower bound",
      );
      sample(60, 100);
      assert.equal(
        resolution.scale,
        1,
        "resolution recovers fully even after reaching the floor",
      );
      sample(15, 3);
      for (let i = 0; i < 10; i++) {
        sample(60, 4);
        sample(35, 1);
      }
      assert.equal(
        resolution.scale,
        0.9,
        "middle FPS holds resolution and interrupts recovery",
      );
      sample(15, 2);
      resolution.resetSampling();
      sample(15, 1);
      assert.equal(
        resolution.scale,
        0.9,
        "hidden tabs discard pending slow windows",
      );
      sample(60, 4);
      resolution.resetSampling();
      sample(60, 1);
      assert.equal(
        resolution.scale,
        0.9,
        "hidden tabs discard pending healthy windows",
      );
      for (const dpr of [0.4, 0.75, 1, 2, 3]) {
        const bounded = new AdaptiveRenderScale(dpr);
        for (let i = 0; i < 100; i++) bounded.sample(10);
        assert.equal(bounded.scale, Math.min(dpr, 0.5));
        for (let i = 0; i < 100; i++) bounded.sample(60);
        assert.equal(
          bounded.scale,
          Math.min(dpr, 1.5),
          "recovery respects the initial DPR cap",
        );
      }
    }
  });
  test("label placement fades and keeps shown labels", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
    const label = (name: string, depth: number, x = 100, active = false) => ({
      name,
      x,
      y: 50,
      width: 40,
      depth,
      active,
      state: { alpha: 0, shown: false },
    });
    const names = (labels: { name: string }[]) => labels.map((l) => l.name);
    const near = label("near", 0.1),
      far = label("far", 0.2),
      apart = label("apart", 0.3, 300);
    let drawn = placeLabels([far, near, apart], 0.5);
    assert.deepEqual(names(drawn), ["near", "apart"], "nearer label wins");
    assert.equal(near.state.alpha, 0.5, "new labels fade in");
    placeLabels([far, near, apart], 0.5);
    assert.equal(near.state.alpha, 1);
    near.depth = 0.21;
    drawn = placeLabels([far, near, apart], 0.5);
    assert.deepEqual(
      names(drawn),
      ["near", "apart"],
      "a shown label keeps its place when depths nearly swap",
    );
    assert.equal(far.state.alpha, 0);
    apart.x = 110;
    drawn = placeLabels([far, near, apart], 0.5);
    assert.deepEqual(names(drawn), ["near", "apart"]);
    assert.equal(apart.state.alpha, 0.5, "a newly covered label fades out");
    drawn = placeLabels([far, near, apart], 0.5);
    assert.deepEqual(names(drawn), ["near"], "faded labels are not drawn");
    const selected = label("selected", 0.9, 100, true);
    drawn = placeLabels([near, selected], 1);
    assert.deepEqual(
      names(drawn),
      ["selected"],
      "active labels win and fadeStep 1 switches immediately",
    );
    assert.equal(near.state.alpha, 0);
  });
  test("IPC particle plan and CPU glow", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
    assert.deepEqual(ipcParticlePlan(1), { duration: 2000, offsets: [0, 100] });
    assert.deepEqual(ipcParticlePlan(2).offsets, [0, 100, 200]);
    assert.equal(ipcParticlePlan(4).offsets.length, 4);
    assert.equal(ipcParticlePlan(8).offsets.length, 5);
    assert.equal(ipcParticlePlan(16).offsets.length, 6);
    assert.equal(ipcParticlePlan(1_000_000).offsets.length, 6);
    assert.deepEqual(ipcParticlePlan(16, true), {
      duration: 150,
      offsets: [0],
    });
    const active = {
      last: 1000,
      window_ms: 100,
      runtime_ns: 20_000_000,
      running_threads: 1,
      switches: 0,
      cpus: [],
    };
    assert.ok(cpuGlowLevel(active, 1000) > 0.9, "running process is bright");
    assert.ok(
      cpuGlowLevel({ ...active, running_threads: 0 }, 1000) > 0.5,
      "recent runtime is visible",
    );
    assert.ok(
      cpuGlowLevel(active, 1250) < cpuGlowLevel(active, 1000),
      "afterglow fades",
    );
    assert.equal(cpuGlowLevel(active, 1500), 0, "afterglow ends after 500ms");
    assert.equal(
      cpuGlowLevel(active, 999),
      0,
      "future timestamps are rejected",
    );
    assert.equal(cpuGlowLevel(undefined, 1000), 0);
  });
}
