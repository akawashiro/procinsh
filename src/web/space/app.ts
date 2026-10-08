// Compose SPACE data, rendering, and user actions; own page lifecycle events.
import * as T from "/vendor/three.module.js";
import { SpaceDataStore } from "./data.js";
import type { ActivityUpdate } from "./data.js";
import type { SystemSnapshot, SpaceActivity } from "../shared/api-types.js";
import { createSpaceScene } from "./scene.js";
import { createSpaceRenderer } from "./renderer.js";
import { createSpaceCamera } from "./camera.js";
import { createSpaceSearch } from "./search.js";
import { SpaceSelection, bindSelection } from "./selection.js";
import type { SelectionActions } from "./selection.js";
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
    scene.rebuild();
    renderer.retainParticles();
  },
  (id) => selectProcess(id, true),
);
const scene = createSpaceScene(data, selection, search.visibleIds);
const camera = createSpaceCamera($("world"), data);
const renderer = createSpaceRenderer({
  graphics: T,
  canvas: $("world"),
  labelCanvas: $("labels"),
  fpsLabel: $("fps"),
  failure: $("failure"),
  data,
  view: scene,
  cameraController: camera,
  selection,
  pruneFiles,
});
const actions: SelectionActions = {
  process: selectProcess,
  connection: selectConnection,
  network: selectNetwork,
  file: selectFile,
  clear: clearSelection,
};
const details = createSpaceDetails(data, selection, actions);
bindSelection($("world"), $("hover"), selection, data, scene, camera, actions);
let firstView = true;

function updateSelection() {
  scene.updateSelection();
  details.update();
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
  selection.retain(data);
  scene.rebuild();
  renderer.retainParticles();
  camera.adaptWorld(scene.root);
  if (firstView && data.nodes.size) {
    camera.fit();
    firstView = false;
  }
  details.update();
}
function renderSystemSnapshot(snapshot: SystemSnapshot, rearrange = false) {
  data.replaceSnapshot(snapshot, rearrange);
  updateSnapshot();
}
function refreshFiles() {
  selection.retain(data);
  scene.refreshFiles();
  renderer.retainParticles();
  camera.adaptWorld(scene.root);
  details.update();
}
function updateActivity(update: ActivityUpdate) {
  if (update.filesChanged) refreshFiles();
  renderer.activity(update);
  details.update();
}
function renderActivity(activity: SpaceActivity) {
  updateActivity(data.ingestActivity(activity, search.visibleIds()));
}
function pruneFiles(now = performance.now()) {
  if (data.pruneFiles(now)) refreshFiles();
}
function fitScene() {
  camera.fit();
}

$("rearrange").onclick = () => {
  renderer.clearParticles();
  renderSystemSnapshot(data.snapshot, true);
  camera.fit();
};
$("close").onclick = clearSelection;
$("reset").onclick = () => {
  search.clear();
  camera.fit();
  clearSelection();
  scene.rebuild();
  renderer.retainParticles();
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
