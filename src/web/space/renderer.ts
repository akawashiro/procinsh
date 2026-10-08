// Animate activity, draw the scene and labels, and adapt rendering resolution.
import type * as T from "/vendor/three.module.js";
import type {
  ActivityUpdate,
  CpuGlow,
  SelectionState,
  HoverState,
} from "./types.js";
import type { RenderView } from "./contracts.js";
// Each sample covers roughly one second of visible rendering. Separate thresholds
// and consecutive windows keep transient scene rebuilds from changing resolution.
export class AdaptiveRenderScale {
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

export const IPC_PARTICLE_DURATION_MS = 2000;
export const IPC_PARTICLE_STAGGER_MS = 100;
export const REDUCED_MOTION_PARTICLE_DURATION_MS = 150;

export function ipcParticlePlan(operationCount: number, reducedMotion = false) {
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

export function cpuGlowLevel(
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

type Particle = {
  start: number;
  duration: number;
  color: number;
  networkId?: string;
  fileId?: string;
} & (
  | { curve: T.QuadraticBezierCurve3; direction: number | null; pos?: never }
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
  const labelPoint = new graphics.Vector3();
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
  scene.add(points);
  function resizeLabels() {
    const ratio = Math.min(devicePixelRatio, 2);
    labelCanvas.width = Math.round(innerWidth * ratio);
    labelCanvas.height = Math.round(innerHeight * ratio);
    labelContext.setTransform(ratio, 0, 0, ratio, 0, 0);
  }
  function drawLabels() {
    const { nodes, hullIds, networkViews, fileViews } = readView();
    const {
      process: selected,
      connection: selectedEdge,
      network: selectedNetwork,
      file: selectedFile,
      hoveredNetwork,
      hoveredFile,
    } = readSelection();
    labelContext.clearRect(0, 0, innerWidth, innerHeight);
    labelContext.font =
      "10px ui-monospace, SFMono-Regular, Consolas, monospace";
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
        v.pos.clone().add(new graphics.Vector3(0, 0, 1)),
        v.group.id === selectedNetwork ||
          v.group.id === hoveredNetwork ||
          v.group.members.some((e) => e.id === selectedEdge),
        true,
      );
    for (const v of fileViews.values())
      add(
        v.file.label,
        v.pos.clone().add(new graphics.Vector3(0, 0, 0.7)),
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
            pos: n.pos.clone().add(new graphics.Vector3(1.2, 0, 1)),
            color: 0xffffff,
          });
        continue;
      }
      const path =
        route.kind === "file"
          ? view.fileViews.get(route.id)
          : view.edgeViews.find((v) => v.e.id === route.id);
      if (!path) continue;
      const networkId =
        route.kind === "connection" && "networkId" in path
          ? path.networkId
          : undefined;
      for (const offset of plan.offsets)
        particles.push({
          start: now + offset,
          duration: plan.duration,
          curve: path.curve,
          direction: route.direction ?? null,
          fileId: route.kind === "file" ? route.id : undefined,
          networkId,
          color: 0xffffff,
        });
    }
    if (particles.length > CAP) particles = particles.slice(-CAP);
  }
  function retainParticles(networkIds: ReadonlySet<string>) {
    const view = readView();
    particles = particles.filter(
      (p) =>
        (!p.fileId || view.fileViews.has(p.fileId)) &&
        (!p.networkId || networkIds.has(p.networkId)),
    );
  }
  function animate(now: number) {
    if (!running) return;
    frame = requestAnimationFrame(animate);
    if (document.hidden) return;
    beforeFrame(now);
    particles = particles.filter((p) => now - p.start < p.duration);
    let i = 0;
    for (const p of particles) {
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
    pg.setDrawRange(0, i);
    pg.attributes.position.needsUpdate = true;
    pg.attributes.color.needsUpdate = true;
    const { baseGlow, haloGlow, hullIds } = readView();
    if (baseGlow && haloGlow) {
      for (let j = 0; j < hullIds.length; j++) {
        const level = cpuGlowLevel(cpuGlows.get(hullIds[j]), now);
        baseGlow.setColorAt(
          j,
          new graphics.Color().setRGB(0.45 * level, 1.15 * level, 1.05 * level),
        );
        haloGlow.setColorAt(
          j,
          new graphics.Color().setRGB(0.18 * level, 0.85 * level, 0.72 * level),
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
      const previousScale = renderResolution.scale;
      const renderScale = renderResolution.sample(fps);
      if (renderScale !== previousScale) renderer.setPixelRatio(renderScale);
      fpsLabel.textContent = `${fps} FPS · ${Math.round(renderScale * 100)}% render`;
      frames = 0;
      frameTime = now;
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
  function resetRenderSampling() {
    frames = 0;
    frameTime = performance.now();
    renderResolution.resetSampling();
  }

  return {
    activity,
    retainParticles,
    networkVisuals,
    networkParticles,
    fileVisuals,
    fileParticles,
    resetSampling: resetRenderSampling,
    clearParticles() {
      particles = [];
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
      particleTexture.dispose();
      scene.remove(points);
      renderer.dispose();
    },
  };
}
export type SpaceRenderer = ReturnType<typeof createSpaceRenderer>;
