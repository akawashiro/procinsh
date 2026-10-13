// Compose SPACE data, rendering, and user actions; own page lifecycle events.
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
import { initGlyph3D, ProcessInspectionCard } from "./inspection-card.js";

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
let inspectionCard: ProcessInspectionCard | null = null;
initGlyph3D()
  .then((atlas) => {
    if (atlas) {
      inspectionCard = new ProcessInspectionCard(atlas);
      scene.root.add(inspectionCard);
    }
  })
  .catch((err) => {
    console.warn("Failed to initialize inspection card:", err);
  });

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
    inspectionCard?.updateCamera(camera.camera);
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
  onRegionHover(target) {
    inspectionCard?.handleHover(target, camera.camera);
  },
  hitInspectionCard(ray) {
    const hit = inspectionCard ? inspectionCard.hitTest(ray) : false;
    if (hit) inspectionCard?.keepAlive();
    return hit;
  },
  onInspectionCardClick() {
    inspectionCard?.toggleLock();
  },
});

window.addEventListener("keydown", (e) => {
  const isInput =
    document.activeElement?.tagName === "INPUT" ||
    document.activeElement?.tagName === "TEXTAREA";
  if (isInput) return;

  if (e.key === "x" || e.key === "X" || e.key === "d" || e.key === "D") {
    inspectionCard?.keepAlive();
    inspectionCard?.toggleMode(camera.camera);
  } else if (e.key === "Escape") {
    inspectionCard?.unlock();
  } else if (e.key === "l" || e.key === "L") {
    inspectionCard?.keepAlive();
    inspectionCard?.toggleLock();
  } else if (
    e.key === "+" ||
    e.key === "=" ||
    e.code === "Equal" ||
    e.code === "NumpadAdd"
  ) {
    e.preventDefault();
    inspectionCard?.keepAlive();
    inspectionCard?.increaseSlicesPerPage(camera.camera);
  } else if (
    e.key === "-" ||
    e.key === "_" ||
    e.code === "Minus" ||
    e.code === "NumpadSubtract"
  ) {
    e.preventDefault();
    inspectionCard?.keepAlive();
    inspectionCard?.decreaseSlicesPerPage(camera.camera);
  } else if (e.key === "s" || e.key === "S" || e.code === "KeyS") {
    e.preventDefault();
    inspectionCard?.keepAlive();
    if (e.shiftKey) {
      inspectionCard?.decreaseSlicesPerPage(camera.camera);
    } else {
      inspectionCard?.increaseSlicesPerPage(camera.camera);
    }
  } else if (
    e.key === "m" ||
    e.key === "M" ||
    e.code === "KeyM" ||
    e.key === "h" ||
    e.key === "H" ||
    e.code === "KeyH" ||
    e.key === "b" ||
    e.key === "B" ||
    e.code === "KeyB"
  ) {
    e.preventDefault();
    inspectionCard?.keepAlive();
    inspectionCard?.toggleMappingRows(camera.camera);
  } else if (
    e.key === "[" ||
    e.key === "PageUp" ||
    e.key === "ArrowUp" ||
    e.key === "k" ||
    e.key === "K"
  ) {
    e.preventDefault();
    inspectionCard?.keepAlive();
    inspectionCard?.scrollPages(-1);
  } else if (
    e.key === "]" ||
    e.key === "PageDown" ||
    e.key === "ArrowDown" ||
    e.key === "j" ||
    e.key === "J"
  ) {
    e.preventDefault();
    inspectionCard?.keepAlive();
    inspectionCard?.scrollPages(1);
  }
});

const wheelRay = new T.Raycaster();
const wheelMouse = new T.Vector2();

$("world").addEventListener(
  "wheel",
  (e: WheelEvent) => {
    if (!inspectionCard?.isInteractive() || e.ctrlKey || e.metaKey) return;
    wheelMouse.set(
      (e.clientX / innerWidth) * 2 - 1,
      (-e.clientY / innerHeight) * 2 + 1,
    );
    wheelRay.setFromCamera(wheelMouse, camera.camera);
    if (inspectionCard.hitTest(wheelRay)) {
      e.preventDefault();
      e.stopPropagation();
      if (e.shiftKey) {
        if (e.deltaY > 0) {
          inspectionCard.increaseSlicesPerPage(camera.camera);
        } else {
          inspectionCard.decreaseSlicesPerPage(camera.camera);
        }
      } else {
        inspectionCard.scrollPages(e.deltaY > 0 ? 1 : -1);
      }
    }
  },
  { passive: false },
);

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
  inspectionCard?.unlock();
}
function selectProcess(id: string | null, focus = false) {
  selection.choose("process", id);
  updateSelection();
  const pos = id && data.nodes.get(id)?.pos;
  if (focus && pos) camera.focus(pos);
  if (id && inspectionCard?.hasTarget(id)) {
    inspectionCard.lock();
  }
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
