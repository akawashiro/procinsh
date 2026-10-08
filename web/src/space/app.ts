// Compose SPACE data, rendering, and user actions; own page lifecycle events.
import * as T from "three";
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

const selection = new SpaceSelection();
const data = new SpaceDataStore(
  {
    snapshot: updateSnapshot,
    activity: updateActivity,
    reset() {
      renderer.clearParticles();
      refreshFiles();
    },
    gap() {
      renderer.clearParticles();
    },
    status(message) {
      $("failure").hidden = message === null;
      $("failure").textContent = message;
    },
  },
  () => search.visibleIds(),
);
const search = createSpaceSearch(
  $("search"),
  () => data.snapshot,
  () => {
    renderer.clearParticles();
    rebuildScene();
    retainParticles();
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
  beforeFrame(now) {
    camera.controls.update();
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
  clear: clearSelection,
};
const details = createSpaceDetails(actions);
bindSelection($("world"), $("hover"), {
  camera: camera.camera,
  read: () => ({
    view: scene.pickingView(),
    edgeStats: data.edgeStats,
    files: data.recentFiles.entries,
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
    snapshot: data.snapshot,
    nodes: data.nodes,
    network: data.network,
    networkPositions: data.networkPositions,
    files: data.recentFiles.entries,
    filePositions: data.filePositions,
    visible: search.visibleIds(),
  };
}
function detailsInput(): DetailsInput {
  return {
    nodes: data.nodes,
    snapshot: data.snapshot,
    network: data.network,
    edgeStats: data.edgeStats,
    files: data.recentFiles.entries,
  };
}
function worldPositions(): WorldPositions {
  return {
    processes: [...data.nodes.values()].map((n) => n.pos),
    networks: data.networkPositions.values(),
    files: data.filePositions.values(),
  };
}
function retainSelection() {
  selection.retain({
    processes: new Set(data.nodes.keys()),
    connections: new Set(data.snapshot.fd_relations.map((e) => e.id)),
    networks: new Set(data.network.keys()),
    files: new Set(data.recentFiles.entries.keys()),
  });
}
function retainParticles() {
  renderer.retainParticles(new Set(data.network.keys()));
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
function updateActivity(update: ActivityUpdate) {
  if (update.filesChanged) refreshFiles();
  renderer.activity(update);
  updateDetails();
}
function renderActivity(activity: SpaceActivity) {
  updateActivity(data.ingestActivity(activity, search.visibleIds()));
}
function pruneFiles(now = performance.now()) {
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
  fitScene();
  clearSelection();
  rebuildScene();
  retainParticles();
};
addEventListener("resize", () => {
  renderer.resize();
  camera.resize();
});
function start() {
  if (document.hidden) return;
  renderer.resume();
  data.start();
}
function stop() {
  renderer.pause();
  data.stop();
}
document.addEventListener("visibilitychange", () => {
  if (document.hidden) stop();
  else start();
});
addEventListener("pagehide", stop);
addEventListener("pageshow", (event) => {
  if (event.persisted) start();
});
start();

const cpuGlowStates = data.cpuGlows;
const cpuGlowVisual = scene.cpuGlowVisual,
  parentLineVisual = scene.parentLineVisual;
const networkVisuals = renderer.networkVisuals,
  networkParticles = renderer.networkParticles,
  fileVisuals = renderer.fileVisuals,
  fileParticles = renderer.fileParticles;
const cameraView = camera.view;
function processPosition(id: string) {
  const pos = data.nodes.get(id)?.pos;
  return pos && { x: pos.x, y: pos.y, z: pos.z };
}
export {
  selectFile,
  fileVisuals,
  fileParticles,
  pruneFiles,
  renderSystemSnapshot,
  renderActivity,
  fitScene,
  selectProcess,
  selectConnection,
  selectNetwork,
  networkVisuals,
  networkParticles,
  cameraView,
  processPosition,
  parentLineVisual,
  cpuGlowStates,
  cpuGlowVisual,
};

// Expose the same model and render-scale helpers to bundled browser inspections.
export { key, treeLayout } from "./data.js";
export { AdaptiveRenderScale } from "./renderer.js";
