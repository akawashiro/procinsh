import { describe, expect, test } from "vitest";
import * as T from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { TourCamera } from "../src/space/tour-camera.js";
import type { TourScene, CpuGlow } from "../src/space/types.js";
import type { RenderView } from "../src/space/contracts.js";
import { tourScene } from "../src/space/tour-scene.js";
import {
  processInfo,
  fdEndpoint,
  fdRelation,
  cpuActivity,
} from "./support/fixtures.js";

function setup(scene: TourScene = { subjects: [], connections: [] }) {
  const camera = new T.PerspectiveCamera(45, 1.6, 0.1, 1500);
  camera.up.set(0, 0, 1);
  camera.position.set(65, -85, 70);
  const controls = new OrbitControls(camera);
  controls.enableDamping = true;
  controls.maxPolarAngle = Math.PI * 0.49;
  controls.minDistance = 0.35;
  controls.maxDistance = 500;
  let samples = 0;
  let rearranges = 0;
  const changes: boolean[] = [];
  const tour = new TourCamera(
    camera,
    controls,
    () => {
      samples++;
      return scene;
    },
    (active) => changes.push(active),
    () => {
      rearranges++;
    },
    () => 0.5,
  );
  let time = 0;
  function advance(seconds: number) {
    for (let i = 0; i < Math.ceil(seconds * 60); i++) {
      time += 1000 / 60;
      tour.update(time);
    }
  }
  return {
    camera,
    controls,
    tour,
    changes,
    advance,
    now: () => time,
    samples: () => samples,
    rearranges: () => rearranges,
  };
}
const busy: TourScene = {
  subjects: [
    {
      id: "quiet",
      position: { x: -12, y: 0, z: 4 },
      activity: 0,
      connections: 0,
    },
    {
      id: "busy",
      position: { x: 12, y: 0, z: 4 },
      activity: 0.8,
      connections: 5,
    },
  ],
  connections: [
    [
      { x: -12, y: 0, z: 4 },
      { x: 0, y: 0, z: 12 },
      { x: 12, y: 0, z: 4 },
    ],
  ],
};

describe("SPACE cinematic tour", () => {
  test("reduced motion, explicit start/stop and control interruption preserve manual control", () => {
    const { tour, camera, controls, changes, advance } = setup(busy);
    tour.start(true);
    expect(tour.active).toBe(false);
    expect(changes).toEqual([]);
    const initial = camera.position.clone();
    tour.start();
    tour.update(0);
    expect(camera.position.distanceTo(initial)).toBeLessThan(1e-10);
    expect(controls.enableDamping).toBe(false);
    advance(4);
    expect(camera.position.distanceTo(initial)).toBeGreaterThan(1);
    const interrupted = camera.position.clone();
    const target = controls.target.clone();
    controls.dispatchEvent({ type: "start" });
    expect(tour.active).toBe(false);
    expect(controls.enableDamping).toBe(true);
    advance(20);
    expect(camera.position.equals(interrupted)).toBe(true);
    expect(controls.target.equals(target)).toBe(true);
    expect(changes).toEqual([true, false]);
    tour.start();
    tour.stop();
    expect(changes).toEqual([true, false, true, false]);
  });

  test("three compositions recur in order, sampling only at transitions and periodically rearranging", () => {
    const s = setup(busy);
    s.tour.start();
    const kinds = new Set<string>();
    const order: string[] = [];
    for (let i = 0; i < 480; i++) {
      kinds.add(s.tour.shotKind!);
      if (order.at(-1) !== s.tour.shotKind) order.push(s.tour.shotKind!);
      s.advance(1);
      expect(
        [...s.camera.position, ...s.controls.target].every(Number.isFinite),
      ).toBe(true);
      expect(s.camera.position.z).toBeGreaterThan(0);
      expect(
        s.camera.position.distanceTo(s.controls.target),
      ).toBeLessThanOrEqual(500.0001);
    }
    expect([...kinds].sort()).toEqual(["flythrough", "hero", "overview"]);
    expect(order.slice(0, 7)).toEqual([
      "overview",
      "hero",
      "flythrough",
      "overview",
      "hero",
      "flythrough",
      "overview",
    ]);
    expect(s.samples()).toBeGreaterThan(12);
    expect(s.samples()).toBeLessThan(35);
    expect(s.rearranges()).toBeGreaterThanOrEqual(2);
    expect(s.tour.active).toBe(true);
  });

  test("overview traverses a full circle at a fixed radius, height and target", () => {
    const s = setup(busy);
    s.tour.start();
    let arrival: T.Vector3 | undefined;
    let target: T.Vector3 | undefined;
    let previousAngle: number | undefined;
    let swept = 0;
    let radius = 0;
    // With the fixed random source, travel is 10 and orbit is 16 tour seconds.
    for (let i = 0; i <= 2600; i++) {
      s.tour.update((i * 1000) / 140);
      if (i < 1000) continue;
      const offset = s.camera.position.clone().sub(s.controls.target);
      const angle = Math.atan2(offset.y, offset.x);
      if (previousAngle !== undefined) {
        const delta = Math.atan2(
          Math.sin(angle - previousAngle),
          Math.cos(angle - previousAngle),
        );
        expect(delta).toBeGreaterThanOrEqual(-1e-10);
        expect(delta).toBeLessThan(0.02);
        swept += delta;
      } else {
        arrival = s.camera.position.clone();
        target = s.controls.target.clone();
        radius = Math.hypot(offset.x, offset.y);
        expect(Math.atan2(offset.z, radius)).toBeLessThan(Math.PI / 6);
      }
      expect(s.controls.target.distanceTo(target!)).toBeLessThan(1e-9);
      expect(Math.hypot(offset.x, offset.y)).toBeCloseTo(radius, 8);
      expect(s.camera.position.z).toBeCloseTo(arrival!.z, 8);
      previousAngle = angle;
    }
    expect(swept).toBeCloseTo(Math.PI * 2, 8);
    expect(s.camera.position.distanceTo(arrival!)).toBeLessThan(1e-8);
  });

  test("manual control can interrupt a full overview orbit without a jump", () => {
    const s = setup(busy);
    s.tour.start();
    s.advance(12);
    expect(s.tour.shotKind).toBe("overview");
    const position = s.camera.position.clone();
    const target = s.controls.target.clone();
    s.controls.dispatchEvent({ type: "start" });
    s.controls.update();
    s.advance(20);
    expect(s.tour.active).toBe(false);
    expect(s.camera.position.distanceTo(position)).toBeLessThan(1e-9);
    expect(s.controls.target.distanceTo(target)).toBeLessThan(1e-9);
  });

  test("flythrough visits userspace subjects at constant speed for 15 seconds", () => {
    const s = setup({
      connections: [],
      subjects: Array.from({ length: 6 }, (_, i) => ({
        id: `userspace-${i}`,
        position: { x: 100 + i * 6, y: (i % 3) * 5, z: 4 },
        activity: 0,
        connections: 0,
      })),
    });
    s.tour.start();
    for (let i = 0; s.tour.shotKind !== "flythrough" && i < 5000; i++)
      s.advance(1 / 60);
    expect(s.tour.shotKind).toBe("flythrough");
    s.advance(10 / 1.4 + 0.2);
    expect(s.controls.target.x).toBeGreaterThan(90);
    const previous = s.camera.position.clone();
    const sampleCount = s.samples();
    for (let i = 0; i < 360; i++) {
      s.advance(1 / 60);
      const speed = s.camera.position.distanceTo(previous) * 60;
      expect(speed).toBeGreaterThan(5.8);
      expect(speed).toBeLessThan(6.2);
      previous.copy(s.camera.position);
    }
    expect(s.samples()).toBe(sampleCount);
    s.advance(8.6);
    expect(s.tour.shotKind).toBe("flythrough");
    s.advance(0.4);
    expect(s.tour.shotKind).toBe("overview");
  });

  test("flythrough safely resumes after hiding and manual interruption freezes its pose", () => {
    const s = setup(busy);
    s.tour.start();
    s.advance(52 / 1.4 + 10 / 1.4 + 3);
    expect(s.tour.shotKind).toBe("flythrough");
    const position = s.camera.position.clone();
    const target = s.controls.target.clone();
    s.tour.suspend();
    s.tour.update(s.now() + 3600000);
    expect(s.camera.position.distanceTo(position)).toBeLessThan(1e-9);
    s.controls.dispatchEvent({ type: "start" });
    s.controls.update();
    s.advance(10);
    expect(s.tour.active).toBe(false);
    expect(s.camera.position.distanceTo(position)).toBeLessThan(1e-9);
    expect(s.controls.target.distanceTo(target)).toBeLessThan(1e-9);
  });

  test("hero compositions favor connected activity, then avoid the previous subject", () => {
    const s = setup(busy);
    s.tour.start();
    s.advance(44 / 1.4);
    expect(s.tour.shotKind).toBe("hero");
    expect(s.controls.target.x).toBeCloseTo(12);
    s.advance(83 / 1.4);
    expect(s.tour.shotKind).toBe("hero");
    expect(s.controls.target.x).toBeCloseTo(-12);
    // A frozen scene does not need activity updates to keep the tour running.
    s.advance(100);
    expect(s.tour.active).toBe(true);
  });

  test("hidden-tab resume and large frame gaps never fast-forward or retarget", () => {
    const s = setup(busy);
    s.tour.start();
    s.advance(5);
    const before = s.camera.position.clone();
    const samples = s.samples();
    s.tour.suspend();
    s.tour.update(s.now() + 3600000);
    expect(s.camera.position.distanceTo(before)).toBeLessThan(1e-10);
    s.tour.update(s.now() + 7200000);
    expect(s.camera.position.distanceTo(before)).toBeLessThan(1);
    expect(s.samples()).toBe(samples);
    s.tour.update(Number.NaN);
    expect([...s.camera.position].every(Number.isFinite)).toBe(true);
  });

  test.each([0, 1, 1000])(
    "%i quiet subjects and portrait view remain finite and smooth across shot boundaries",
    (count) => {
      const s = setup({
        subjects: Array.from({ length: count }, (_, i) => ({
          id: String(i),
          position: { x: i % 32, y: Math.floor(i / 32), z: 4 },
          activity: 0,
          connections: 0,
        })),
        connections: [],
      });
      s.camera.aspect = 0.5;
      s.tour.start();
      let previous = s.camera.position.clone();
      for (let i = 0; i < 12000; i++) {
        s.tour.update((i * 1000) / 60);
        expect(
          [...s.camera.position, ...s.controls.target].every(Number.isFinite),
        ).toBe(true);
        expect(s.camera.position.distanceTo(s.controls.target)).toBeGreaterThan(
          1,
        );
        expect(s.camera.position.distanceTo(previous)).toBeLessThan(3);
        previous.copy(s.camera.position);
      }
      expect(s.tour.active).toBe(true);
    },
  );

  test("lingers on some compositions and freezes the final pose on exit", () => {
    const s = setup(busy);
    // Default deterministic shots have no hold; supply a random value that does.
    const tour = new TourCamera(
      s.camera,
      s.controls,
      () => busy,
      () => {},
      () => {},
      () => 0.25,
    );
    tour.start();
    for (let i = 0; i <= 2300; i++) tour.update((i * 10) / 1.4);
    const pose = s.camera.position.clone();
    tour.update(23100 / 1.4);
    tour.update(23200 / 1.4);
    expect(s.camera.position.distanceTo(pose)).toBeLessThan(1e-10);
    tour.stop();
    s.controls.update();
    expect(s.camera.position.distanceTo(pose)).toBeLessThan(1e-10);
  });
});

test("tour scene samples visible towers, real connection paths and recent CPU activity", () => {
  const curve = new T.QuadraticBezierCurve3(
    new T.Vector3(-12, 0, 4),
    new T.Vector3(0, 0, 8),
    new T.Vector3(12, 0, 4),
  );
  const view: RenderView = {
    nodes: new Map(
      [1, 2, 3].map((pid) => [
        String(pid) + ":1",
        {
          ...processInfo({ identity: { pid, start_time_ticks: 1 } }),
          pos: new T.Vector3(pid * 12, 0, 0),
          regions: [],
        },
      ]),
    ),
    hullIds: ["1:1", "2:1"],
    edgeViews: [
      {
        e: fdRelation({
          endpoint: fdEndpoint({ process_id: { pid: 1, start_time_ticks: 1 } }),
        }),
        curve,
      },
    ],
    networkViews: [],
    fileViews: new Map(),
    netlinkViews: new Map(),
    baseGlow: null,
    haloGlow: null,
  };
  const cpu = new Map<string, CpuGlow>([
    [
      "1:1",
      { ...cpuActivity({ runtime_ns: 50000000 }), last: 1000, window_ms: 100 },
    ],
  ]);
  const scene = tourScene(view, cpu, 1100);
  expect(scene.subjects.map((s) => s.id)).toEqual(["1:1", "2:1"]);
  expect(scene.subjects[0]).toMatchObject({
    position: { x: 12, y: 0, z: 4 },
    activity: 0.5,
    connections: 1,
  });
  expect(scene.subjects[1].activity).toBe(0);
  expect(scene.connections[0][0]).toEqual(curve.getPoint(0));
  expect(scene.connections[0].at(-1)).toEqual(curve.getPoint(1));
  expect(tourScene(view, cpu, 10000).subjects[0].activity).toBe(0);
  expect(tourScene(view, cpu, 0).subjects[0].activity).toBe(0);
});

test("tour favors the systemd tree and excludes kthreadd descendants and their connections", () => {
  const specs = [
    { pid: 1, name: "systemd", parent: null },
    { pid: 2, name: "kthreadd", parent: null },
    { pid: 3, name: "kworker", parent: 2 },
    { pid: 4, name: "nested-worker", parent: 3 },
    { pid: 5, name: "service", parent: 1 },
    { pid: 6, name: "chromium", parent: 5 },
    { pid: 7, name: "orphan", parent: null },
  ];
  const identity = (pid: number) => ({ pid, start_time_ticks: 1 });
  const curve = new T.QuadraticBezierCurve3(
    new T.Vector3(),
    new T.Vector3(1, 1, 8),
    new T.Vector3(2, 2, 4),
  );
  const view: RenderView = {
    nodes: new Map(
      specs.map(({ pid, name, parent }) => [
        `${pid}:1`,
        {
          ...processInfo({
            identity: identity(pid),
            name,
            parent_id: parent === null ? null : identity(parent),
          }),
          pos: new T.Vector3(pid * 10, 0, 0),
          regions: [],
        },
      ]),
    ),
    hullIds: specs.map(({ pid }) => `${pid}:1`),
    edgeViews: [
      [5, 6],
      [3, 4],
      [6, 3],
    ].map(([from, to]) => ({
      e: fdRelation({
        endpoint: fdEndpoint({ process_id: identity(from) }),
        peer: fdEndpoint({ process_id: identity(to) }),
      }),
      curve,
    })),
    networkViews: [],
    fileViews: new Map(),
    netlinkViews: new Map(),
    baseGlow: null,
    haloGlow: null,
  };
  const sample = tourScene(view, new Map(), 1000);
  expect(sample.subjects.map((s) => s.id)).toEqual(["1:1", "5:1", "6:1"]);
  expect(sample.connections).toHaveLength(1);
  expect(sample.subjects.find((s) => s.id === "6:1")?.connections).toBe(1);
  // Hidden ancestors still classify their descendants; filtered or non-systemd
  // recordings fall back to other visible userspace processes.
  view.hullIds = ["3:1", "4:1", "7:1"];
  expect(tourScene(view, new Map(), 1000).subjects.map((s) => s.id)).toEqual([
    "7:1",
  ]);
  view.hullIds = ["3:1", "4:1"];
  expect(tourScene(view, new Map(), 1000)).toEqual({
    subjects: [],
    connections: [],
  });
});
