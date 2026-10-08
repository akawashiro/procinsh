// Own camera projection, world bounds, focus, and OrbitControls.
import * as T from "/vendor/three.module.js";
import { OrbitControls } from "/vendor/OrbitControls.js";
import type { SpaceDataStore, Position } from "./data.js";
export function createSpaceCamera(
  canvas: HTMLCanvasElement,
  data: SpaceDataStore,
) {
  const camera = new T.PerspectiveCamera(
    45,
    innerWidth / innerHeight,
    0.1,
    1500,
  );
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

  function worldBounds() {
    const box = new T.Box3();
    for (const n of data.nodes.values()) {
      box.expandByPoint(new T.Vector3(n.pos.x, n.pos.y, n.pos.z));
      box.expandByPoint(new T.Vector3(n.pos.x, n.pos.y, 8));
    }
    for (const p of data.networkPositions.values())
      box.expandByPoint(new T.Vector3(p.x, p.y, p.z + 1));
    for (const p of data.filePositions.values())
      box.expandByPoint(new T.Vector3(p.x, p.y, p.z - 1));
    return box;
  }
  function adaptWorld(scene: T.Scene) {
    if (!data.nodes.size) return;
    const size = worldBounds().getSize(new T.Vector3()),
      extent = Math.max(50, size.x, size.y, size.z);
    controls.maxDistance = Math.max(500, extent * 2);
    camera.far = Math.max(1500, extent * 4);
    camera.updateProjectionMatrix();
    (scene.fog as T.FogExp2).density = Math.min(0.0015, 0.8 / extent);
  }
  function fit() {
    if (!data.nodes.size) return;
    const box = worldBounds(),
      center = box.getCenter(new T.Vector3()),
      size = box.getSize(new T.Vector3());
    const distance = Math.max(size.x, size.y, size.z) * 0.8 + 20;
    controls.target.copy(center);
    camera.position
      .copy(center)
      .add(new T.Vector3(distance * 0.55, -distance * 0.8, distance * 0.85));
  }
  function cameraView() {
    return {
      position: camera.position.toArray(),
      target: controls.target.toArray(),
    };
  }

  return {
    camera,
    controls,
    adaptWorld,
    fit,
    view: cameraView,
    focus(pos: Position) {
      controls.target.set(pos.x, pos.y, pos.z + 4);
      camera.position.set(pos.x + 13, pos.y - 20, pos.z + 15);
    },
    resize() {
      camera.aspect = innerWidth / innerHeight;
      camera.updateProjectionMatrix();
    },
    dispose() {
      controls.dispose();
    },
  };
}
export type SpaceCamera = ReturnType<typeof createSpaceCamera>;
