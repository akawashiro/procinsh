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
  }),
  actions,
  hover(state) {
    Object.assign(selection, state);
  },
});

let firstView = true;
let lastFilePrune = 0;

function sceneInput(): SceneInput {
  return {
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
    networks: data.networkPositions.values(),
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
  const density = camera.adaptWorld(worldPositions());
  if (density !== undefined) scene.setFogDensity(density);
}
function updateDetails() {
  details.update(detailsInput(), selection);
}

function updateSelection() {
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
    fitScene();
    firstView = false;
  }
  updateDetails();
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
  if (data.nodes.size) camera.fit(worldPositions());
}

$("rearrange").onclick = () => {
  renderer.clearParticles();
  renderSystemSnapshot(data.snapshot, true);
  fitScene();
};
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
  renderer.resume();
  if (!replayMode) data.start();
}
function stop() {
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
