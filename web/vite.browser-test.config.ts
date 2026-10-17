import { resolve } from "node:path";
import { defineConfig, mergeConfig, type Plugin } from "vite";
import applicationConfig from "./vite.config.js";

// Append inspection exports only when building the browser regression fixture.
const inspections = new Map([
  [
    resolve(import.meta.dirname, "src/space/app.ts"),
    `
const cpuGlowStates = data.cpuGlows;
const cpuGlowVisual = scene.cpuGlowVisual,
  parentLineVisual = scene.parentLineVisual;
const networkVisuals = renderer.networkVisuals,
  networkParticles = renderer.networkParticles,
  signalVisuals = renderer.signalVisuals,
  fileVisuals = renderer.fileVisuals,
  fileParticles = renderer.fileParticles,
  netlinkVisuals = renderer.netlinkVisuals,
  netlinkParticles = renderer.netlinkParticles;
const cameraView = camera.view;
const socketVisibilityState = () => ({
  raw: data.snapshot.fd_relations.length,
  shown: scene.renderView().edgeViews.map(view => view.e.id),
  selected: selection.connection,
  particles: renderer.socketParticles(),
});
function processPosition(id: string) {
  const pos = data.nodes.get(id)?.pos;
  return pos && { x: pos.x, y: pos.y, z: pos.z };
}
function ipcPortVisuals() {
  const stems = scene.root.getObjectByName("ipc-port-stems");
  const matrix = new T.Matrix4();
  return Array.from({length:stems.count}, (_, i) => {
    stems.getMatrixAt(i, matrix);
    const normal = new T.Vector3(0,0,1).transformDirection(matrix);
    return {position:new T.Vector3().setFromMatrixPosition(matrix)
      .addScaledVector(normal, stems.geometry.parameters.height / 2).toArray(),
      normal:normal.toArray()};
  });
}
function connectionVisual(id: string) {
  const view = scene.renderView().edgeViews.find(view => view.e.id === id);
  return view && {
    points: view.curve.getPoints(16).map(point => point.toArray()),
    screen: view.curve.getPoint(0.75).project(camera.camera).toArray(),
  };
}
export {
  replayTime,
  socketVisibilityState,
  ipcPortVisuals,
  selectNetlink,
  netlinkVisuals,
  netlinkParticles,
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
  signalVisuals,
  cameraView,
  processPosition,
  connectionVisual,
  parentLineVisual,
  cpuGlowStates,
  cpuGlowVisual,
};

export { key } from "./model.js";
export { treeLayout } from "./process-layout.js";
export { AdaptiveRenderScale } from "./renderer.js";
`,
  ],
  [
    resolve(import.meta.dirname, "src/space/renderer.ts"),
    "export { AdaptiveRenderScale };",
  ],
]);

export default defineConfig((environment) =>
  mergeConfig(applicationConfig(environment), {
    plugins: [
      {
        name: "space-browser-inspections",
        enforce: "pre",
        transform(source, id) {
          const inspection = inspections.get(id);
          if (
            inspection &&
            id === resolve(import.meta.dirname, "src/space/renderer.ts")
          ) {
            source = source.replace(
              "    networkParticles,",
              `    networkParticles,
    socketParticles: () => particles.filter(p => p.socketOwnerKey).map(p => ({connectionId:p.connectionId, socketOwnerKey:p.socketOwnerKey})),`,
            );
          }
          return inspection
            ? { code: `${source}\n${inspection}`, map: null }
            : undefined;
        },
      } satisfies Plugin,
    ],
    build: {
      rolldownOptions: {
        input: { spaceApp: resolve(import.meta.dirname, "src/space/app.ts") },
        preserveEntrySignatures: "exports-only",
      },
    },
  }),
);
