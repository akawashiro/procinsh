// Compose SPACE data, rendering, and user actions; own page lifecycle events.
import { SpaceRecorder } from "./recording.js";
import { watchMasked } from "../shared/privacy.js";
import {
  mountRecordingControls,
  mountReplayControls,
} from "./session-controls.js";
import { setSpaceClock, spaceNow } from "./clock.js";
import * as T from "three";
import { isKernelNetlink } from "./netlink.js";
import { SpaceDataStore } from "./data.js";
import type { ActivityUpdate } from "./types.js";
import type { SystemSnapshot, SpaceActivity } from "../shared/api-types.js";
import { createSpaceScene } from "./scene.js";
import { createSpaceRenderer } from "./renderer.js";
import { createSpaceCamera, type LayerBounds } from "./camera.js";
import { TourCamera } from "./tour-camera.js";
import { tourScene } from "./tour-scene.js";
import { createSpaceSearch } from "./search.js";
import { SpaceSelection, bindSelection } from "./selection.js";
import type {
  SelectionActions,
  SceneInput,
  DetailsInput,
  WorldPositions,
} from "./contracts.js";
import { createSpaceDetails } from "./details.js";
import { ALL_HOSTS, addressHost } from "./hosts.js";
import {
  hostUrl,
  mountHostControls,
  readHostParam,
  withHost,
} from "../shared/hosts.js";
import { spaceElement as $ } from "./dom-types.js";

const replayMode = document.body.dataset.mode === "replay";
let replayTime = 0;
let tickReplay: (() => void) | undefined;
if (replayMode) setSpaceClock(() => replayTime);
const recorder = new SpaceRecorder(spaceNow);
let liveReady = false;

const selection = new SpaceSelection();
const data = new SpaceDataStore(
  {
    snapshot() {
      liveReady = true;
      updateSnapshot();
    },
    activity: updateActivity,
    reset() {
      liveReady = false;
      recorder.stop(
        "Live stream interrupted; download the retained recording.",
      );
      renderer.clearParticles();
      if (data.pruneRetention()) updateSnapshot();
      else refreshFiles();
    },
    gap() {
      recorder.stop("Live event gap; download the retained recording.");
      renderer.clearParticles();
      if (data.pruneRetention()) updateSnapshot();
    },
    status(message) {
      $("failure").hidden = message === null;
      $("failure").textContent = message;
    },
  },
  () => search.visibleIds(),
  undefined,
  (now, suppressed) => renderer.activeResources(now, suppressed),
  (event) => recorder.capture(event),
);
const search = createSpaceSearch(
  $("search"),
  () => data.displaySnapshot,
  () => {
    renderer.clearParticles();
    data.pruneRetention();
    updateSnapshot();
  },
  (id) => selectProcess(id, true),
);
const scene = createSpaceScene();
const camera = createSpaceCamera($("world"));
camera.setLayers(layerBounds);
/** Whether the viewer moved the camera since the last automatic or explicit fit. */
let cameraMoved = false;
/** Host layers framed by the last automatic fit. */
let fittedLayers = 0;
camera.controls.addEventListener("start", () => {
  cameraMoved = true;
});
const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");
let prefersReducedMotion = reducedMotion.matches;
const tour = new TourCamera(
  camera.camera,
  camera.controls,
  () => tourScene(scene.renderView(), data.cpuGlows, spaceNow()),
  (active) => {
    $("tour").textContent = active ? "Exit Tour" : "Tour mode";
    $("tour").setAttribute("aria-pressed", String(active));
    setTourUrl(active);
  },
  rearrangeScene,
);
const renderer = createSpaceRenderer({
  graphics: T,
  canvas: $("world"),
  labelCanvas: $("labels"),
  fpsLabel: $("fps"),
  failure: $("failure"),
  scene: scene.root,
  camera: camera.camera,
  view: scene.renderView,
  selection: () => selection,
  cpuGlows: data.cpuGlows,
  beforeFrame(_now) {
    tickReplay?.();
    const now = spaceNow();
    tour.update(performance.now());
    moveWithKeys(performance.now());
    camera.controls.update();
    if (data.pruneRetention(now)) updateSnapshot();
    if (now - lastFilePrune >= 1000) {
      lastFilePrune = now;
      pruneFiles(now);
    }
  },
});
const actions: SelectionActions = {
  process: selectProcess,
  connection: selectConnection,
  network: selectNetwork,
  file: selectFile,
  netlink: selectNetlink,
  hardware: selectHardware,
  rdma: selectRdma,
  clear: clearSelection,
};
const details = createSpaceDetails(actions);
bindSelection($("world"), $("hover"), {
  camera: camera.camera,
  read: () => ({
    view: scene.pickingView(),
    edgeStats: data.edgeStats,
    files: data.recentFiles.entries,
    netlink: data.netlinkSockets.entries,
    hardware: hardwareInput(),
    rdma: rdmaInput(),
  }),
  actions,
  hover(state) {
    Object.assign(selection, state);
  },
});

let firstView = true;
let lastFilePrune = 0;

function hardwareInput() {
  return {
    devices: data.displaySnapshot.hardware_devices ?? [],
    paths: data.displaySnapshot.hardware_paths ?? [],
    handles: data.displaySnapshot.device_handles ?? [],
  };
}
function rdmaInput() {
  return {
    groups: data.rdma,
    links: data.rdmaLinks,
    devices: data.rdmaDevices,
  };
}
/** Host floor labels of the merged view. */
function hostLabels() {
  return new Map(
    data.hosts.map((h) => [
      h.name,
      `${h.name}${h.local ? " · local" : ""}${h.status.state === "offline" ? " · offline" : ""}`,
    ]),
  );
}
function sceneInput(): SceneInput {
  return {
    hostLabels: hostLabels(),
    hardware: { ...hardwareInput(), positions: data.hardwarePositions },
    rdma: {
      ...rdmaInput(),
      positions: data.rdmaPositions,
      devicePositions: data.rdmaDevicePositions,
    },
    snapshot: data.displaySnapshot,
    nodes: data.nodes,
    network: data.network,
    networkPositions: data.networkPositions,
    files: data.recentFiles.entries,
    netlink: data.netlinkSockets.entries,
    filePositions: data.filePositions,
    netlinkPositions: data.netlinkPositions,
    visible: search.visibleIds(),
  };
}
function detailsInput(): DetailsInput {
  return {
    hardware: hardwareInput(),
    rdma: rdmaInput(),
    // Merged identities carry their host; a single remote host is named here.
    host: data.host === ALL_HOSTS ? null : data.host,
    nodes: data.nodes,
    snapshot: data.displaySnapshot,
    network: data.network,
    edgeStats: data.edgeStats,
    files: data.recentFiles.entries,
    netlink: data.netlinkSockets.entries,
  };
}
function worldPositions(): WorldPositions {
  return {
    processes: [...data.nodes.values()].map((n) => n.pos),
    networks: [
      ...data.networkPositions.values(),
      ...data.hardwarePositions.values(),
      ...data.rdmaPositions.values(),
      ...data.rdmaDevicePositions.values(),
    ],
    files: data.filePositions.values(),
    netlink: data.netlinkPositions.values(),
  };
}
/** The processes and hardware of one host layer, for framing that layer. */
function layerPositions(host: string): WorldPositions {
  return {
    processes: [...data.nodes.values()]
      .filter((n) => n.identity.host === host)
      .map((n) => n.pos),
    networks: [
      ...[...data.hardwarePositions]
        .filter(([address]) => addressHost(address) === host)
        .map(([, p]) => p),
      ...[...data.rdmaDevicePositions]
        .filter(([id]) => data.rdmaDevices.get(id)?.host === host)
        .map(([, p]) => p),
    ],
    files: [],
    netlink: [],
  };
}
/** Host layers top to bottom; rotations pivot halfway up their processes. */
function layerBounds(): LayerBounds[] {
  return [...data.hostLayerZ].flatMap(([host, z]) => {
    const { processes, networks } = layerPositions(host);
    const points = [...processes, ...networks];
    if (!points.length) return [];
    const xs = points.map((p) => p.x),
      ys = points.map((p) => p.y);
    return [
      {
        z: z + 4,
        min: { x: Math.min(...xs), y: Math.min(...ys) },
        max: { x: Math.max(...xs), y: Math.max(...ys) },
      },
    ];
  });
}
/** Frames host layer `index` from the top, or every layer for -1. */
function focusLayer(index: number) {
  const hosts = [...data.hostLayerZ.keys()];
  if (index >= hosts.length) return;
  stopTour();
  camera.fit(index < 0 ? worldPositions() : layerPositions(hosts[index]));
  cameraMoved = true;
}
function retainSelection() {
  selection.retain({
    processes: new Set(data.nodes.keys()),
    connections: new Set(
      data.displaySnapshot.fd_relations
        .filter((e) => !isKernelNetlink(e.socket))
        .map((e) => e.id),
    ),
    networks: new Set(data.network.keys()),
    files: new Set(data.recentFiles.entries.keys()),
    netlink: new Set(data.netlinkSockets.entries.keys()),
    hardware: new Set(data.hardwarePositions.keys()),
    rdma: new Set([
      ...data.rdma.keys(),
      ...data.rdmaLinks.map((l) => l.id),
      ...data.rdmaDevices.keys(),
    ]),
  });
}
function retainParticles() {
  renderer.retainParticles(
    new Set(data.network.keys()),
    data.suppressedSocketOwners,
  );
}
function rebuildScene() {
  scene.rebuild(sceneInput(), selection.process);
  scene.updateSelection(selection);
}
function adaptCamera() {
  if (!data.nodes.size) return;
  const density = camera.adaptWorld(worldPositions(), tour.active);
  if (density !== undefined) scene.setFogDensity(density);
}
function updateDetails() {
  details.update(detailsInput(), selection);
}

function updateSelection() {
  stopTour();
  scene.updateSelection(selection);
  updateDetails();
}
function clearSelection() {
  selection.clear();
  updateSelection();
}
function selectProcess(id: string | null, focus = false) {
  selection.choose("process", id);
  updateSelection();
  const pos = id && data.nodes.get(id)?.pos;
  if (focus && pos) camera.focus(pos);
}
function selectConnection(id: string | null) {
  selection.choose("connection", id);
  updateSelection();
}
function selectNetwork(id: string | null) {
  selection.choose("network", id);
  updateSelection();
}
function selectFile(id: string | null) {
  selection.choose("file", id);
  updateSelection();
}
function updateSnapshot() {
  retainSelection();
  rebuildScene();
  retainParticles();
  adaptCamera();
  if (firstView && data.nodes.size) {
    if (!tour.active) camera.fit(worldPositions());
    firstView = false;
    cameraMoved = false;
    fittedLayers = data.hostLayerZ.size;
  } else if (
    // Hosts of the merged view arrive one by one; frame each new layer until
    // the viewer takes over the camera.
    data.hostLayerZ.size > fittedLayers &&
    !cameraMoved &&
    !tour.active
  ) {
    camera.fit(worldPositions());
    fittedLayers = data.hostLayerZ.size;
  }
  updateDetails();
  syncTourFromUrl();
}
function renderSystemSnapshot(snapshot: SystemSnapshot, rearrange = false) {
  data.replaceSnapshot(snapshot, rearrange);
  updateSnapshot();
}
function refreshFiles() {
  retainSelection();
  scene.refreshFiles(sceneInput());
  scene.updateSelection(selection);
  retainParticles();
  adaptCamera();
  updateDetails();
}
function selectNetlink(id: string | null) {
  selection.choose("netlink", id);
  updateSelection();
}
function selectHardware(id: string | null) {
  selection.choose("hardware", id);
  updateSelection();
}
function selectRdma(id: string | null) {
  selection.choose("rdma", id);
  updateSelection();
}
function updateActivity(update: ActivityUpdate) {
  if (update.filesChanged || update.netlinkChanged) refreshFiles();
  renderer.activity(update);
  updateDetails();
}
function renderActivity(activity: SpaceActivity) {
  updateActivity(data.ingestActivity(activity, search.visibleIds()));
}
function pruneFiles(now = spaceNow()) {
  if (data.pruneFiles(now)) refreshFiles();
}
function fitScene() {
  stopTour();
  if (data.nodes.size) camera.fit(worldPositions());
  cameraMoved = false;
}

function rearrangeScene() {
  renderer.clearParticles();
  renderSystemSnapshot(data.snapshot, true);
}
$("rearrange").onclick = () => {
  stopTour();
  rearrangeScene();
  fitScene();
};
function setTourUrl(active: boolean) {
  const url = new URL(location.href);
  if (active) url.searchParams.set("tour", "1");
  else url.searchParams.delete("tour");
  if (url.href !== location.href) history.replaceState(history.state, "", url);
}
function stopTour() {
  tour.stop();
  setTourUrl(false);
}
function syncTourFromUrl() {
  if (new URL(location.href).searchParams.get("tour") !== "1") {
    tour.stop();
  } else if (prefersReducedMotion) {
    stopTour();
  } else if (data.nodes.size && !document.hidden) {
    tour.start();
  }
}
addEventListener("popstate", syncTourFromUrl);
$("world").addEventListener("pointerdown", stopTour);
$("world").addEventListener("wheel", stopTour, { passive: true });
$("tour").onclick = () => {
  if (tour.active) stopTour();
  else tour.start(prefersReducedMotion);
};
function syncMotionPreference(event?: MediaQueryListEvent) {
  // URL synchronization and input handlers share the preference from the event.
  prefersReducedMotion = event?.matches ?? reducedMotion.matches;
  if (prefersReducedMotion) stopTour();
  $("tour").disabled = prefersReducedMotion;
  $("tour").title = prefersReducedMotion
    ? "Tour motion is disabled by your reduced-motion preference"
    : "Cinematic camera tour · Esc or drag to exit · occasionally rearranges the layout";
}
reducedMotion.addEventListener("change", syncMotionPreference);
syncMotionPreference();
$("search").addEventListener("input", stopTour);
/**
 * Physical keys held down, moving the camera each frame: W/A/S/D slide over the
 * ground plane, arrows orbit. Codes name key positions, so AZERTY's Z/Q/S/D work too.
 */
const heldKeys = new Set<string>();
const heldCodes = [
  "KeyW",
  "KeyA",
  "KeyS",
  "KeyD",
  "ArrowLeft",
  "ArrowRight",
  "ArrowUp",
  "ArrowDown",
];
let fastKeys = false;
let lastMove: number | null = null;
function moveWithKeys(now: number) {
  const elapsed =
    lastMove === null ? 0 : Math.min(0.25, (now - lastMove) / 1000);
  lastMove = heldKeys.size ? now : null;
  if (!heldKeys.size || !elapsed) return;
  const axis = (plus: string, minus: string) =>
    Number(heldKeys.has(plus)) - Number(heldKeys.has(minus));
  // About the target distance per second, so close-ups stay precise; Shift is faster.
  const step = Math.max(5, camera.distance()) * (fastKeys ? 3 : 1) * elapsed;
  camera.move(axis("KeyW", "KeyS") * step, axis("KeyD", "KeyA") * step);
  // A quarter turn per second around the target, turning like a drag in that
  // direction; Up raises the camera towards a top view.
  const turn = (Math.PI / 2) * (fastKeys ? 2 : 1) * elapsed;
  camera.orbit(
    axis("ArrowLeft", "ArrowRight") * turn,
    axis("ArrowUp", "ArrowDown") * turn,
  );
}
function typingIn(target: EventTarget | null) {
  return (
    target instanceof Element &&
    !!target.closest("input, select, textarea, [contenteditable]")
  );
}
addEventListener("keydown", (event) => {
  fastKeys = event.shiftKey;
  if (event.key === "Escape") stopTour();
  if (event.ctrlKey || event.metaKey || event.altKey || typingIn(event.target))
    return;
  if (heldCodes.includes(event.code)) {
    stopTour();
    // Movement starts at the key press, not the next frame, so taps move too.
    if (!heldKeys.size) lastMove = performance.now();
    heldKeys.add(event.code);
    cameraMoved = true;
    event.preventDefault();
  } else if (event.code === "Comma" || event.code === "Period") {
    // The keys right of M zoom in and out, like a wheel step.
    stopTour();
    camera.zoom(event.code === "Comma" ? 0.8 : 1.25);
    cameraMoved = true;
  } else if (/^Digit[0-9]$/.test(event.code) && data.hostLayerZ.size > 1) {
    // 1-9 enter that host layer of the merged view; 0 shows every layer.
    focusLayer(Number(event.code.slice(5)) - 1);
  }
});
addEventListener("keyup", (event) => {
  // Apply the time held since the last frame before the key stops counting.
  if (heldKeys.has(event.code)) moveWithKeys(performance.now());
  fastKeys = event.shiftKey;
  heldKeys.delete(event.code);
});
// Keys released while the page is in the background never send keyup.
addEventListener("blur", () => heldKeys.clear());
$("close").onclick = clearSelection;
$("reset").onclick = () => {
  search.clear();
  renderer.clearParticles();
  data.pruneRetention();
  retainSelection();
  fitScene();
  clearSelection();
  rebuildScene();
  retainParticles();
  updateDetails();
};
addEventListener("resize", () => {
  renderer.resize();
  camera.resize();
});
function start() {
  if (document.hidden) return;
  tour.suspend();
  renderer.resume();
  if (!replayMode) data.start();
  syncTourFromUrl();
}
function stop() {
  tour.suspend();
  renderer.pause();
  recorder.stop("Tab hidden or page left; download the retained recording.");
  if (!replayMode) data.stop();
}
document.addEventListener("visibilitychange", () => {
  if (document.hidden) stop();
  else start();
});
addEventListener("pagehide", stop);
addEventListener("pageshow", (event) => {
  if (event.persisted) start();
});
/** Forgets displayed state before showing an unrelated snapshot source. */
function resetView() {
  renderer.clearParticles();
  data.cpuGlows.clear();
  data.edgeStats.clear();
  data.recentFiles.clear();
  data.netlinkSockets.clear();
  data.nodes.clear();
  data.displaySnapshot = { processes: [], fd_relations: [] };
  data.networkPositions.clear();
  data.hardwarePositions.clear();
  data.rdmaPositions.clear();
  data.rdmaDevicePositions.clear();
  data.filePositions.clear();
  data.netlinkPositions.clear();
  lastFilePrune = 0;
}
/** The list view shows one host; the merged view links to this machine's list. */
function syncListLink() {
  $("back").href = withHost(
    "/list",
    data.host === ALL_HOSTS ? null : data.host,
  );
}
/** Shows another host, or every host as layers, from the live server. */
function switchHost(host: string | null) {
  if (host === data.host) return;
  stopTour();
  data.stop();
  clearSelection();
  resetView();
  data.host = host;
  data.hosts = [];
  data.localHost = null;
  renderSystemSnapshot({ processes: [], fd_relations: [] }, true);
  history.replaceState(history.state, "", hostUrl(host));
  syncListLink();
  firstView = true;
  fittedLayers = 0;
  if (!document.hidden) data.start();
}
if (replayMode) {
  tickReplay = mountReplayControls(
    (snapshot) => {
      resetView();
      renderSystemSnapshot(snapshot, true);
    },
    (event) => {
      if (event.type === "snapshot") renderSystemSnapshot(event.data);
      else renderActivity(event.data);
    },
    (time) => {
      replayTime = time;
    },
  );
} else {
  data.host = readHostParam();
  syncListLink();
  mountRecordingControls(
    recorder,
    () => data.snapshot,
    () => liveReady,
  );
  mountHostControls(
    document.querySelector(".tools")!,
    () => data.host,
    switchHost,
    { all: true },
  );
}
start();
watchMasked();
