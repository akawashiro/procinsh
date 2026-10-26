// Compose SPACE data, rendering, and user actions; own page lifecycle events.
import { SpaceRecorder } from "./recording.js";
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
import { createSpaceCamera } from "./camera.js";
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
function sceneInput(): SceneInput {
  return {
    hardware: { ...hardwareInput(), positions: data.hardwarePositions },
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
    ],
    files: data.filePositions.values(),
    netlink: data.netlinkPositions.values(),
  };
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
addEventListener("keydown", (event) => {
  if (event.key === "Escape") stopTour();
});
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
if (replayMode) {
  tickReplay = mountReplayControls(
    (snapshot) => {
      renderer.clearParticles();
      data.cpuGlows.clear();
      data.edgeStats.clear();
      data.recentFiles.clear();
      data.netlinkSockets.clear();
      data.nodes.clear();
      data.displaySnapshot = { processes: [], fd_relations: [] };
      data.networkPositions.clear();
      data.hardwarePositions.clear();
      data.filePositions.clear();
      data.netlinkPositions.clear();
      lastFilePrune = 0;
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
  mountRecordingControls(
    recorder,
    () => data.snapshot,
    () => liveReady,
  );
}
start();
