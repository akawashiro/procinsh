// @vitest-environment jsdom
import { test, vi, afterEach } from "vitest";
import assert from "node:assert/strict";
import { resolve } from "node:path";
import { readFile, readdir } from "node:fs/promises";
import ts from "typescript";
import * as T from "three";

import type {
  SceneInput,
  PickingInput,
  DetailsInput,
  RenderView,
} from "../src/space/contracts.js";
import type {
  PlacedProcess,
  RecentFile,
  SelectionState,
} from "../src/space/types.js";
import { spaceElement } from "../src/space/dom-types.js";
import {
  processInfo,
  fdEndpoint,
  fdRelation,
  socketEndpoint,
} from "./support/fixtures.js";

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("space-components regression", async () => {
  // Exercise SPACE components with plain inputs, Three.js objects, and callbacks.
  const { createSpaceScene } = await import("../src/space/scene.js");
  const { SpaceSelection, bindSelection } =
    await import("../src/space/selection.js");
  const { createSpaceCamera } = await import("../src/space/camera.js");
  const { createSpaceDetails } = await import("../src/space/details.js");
  const { createSpaceRenderer } = await import("../src/space/renderer.js");

  // Check erased type imports as well as runtime imports to keep the boundary intact.
  for (const name of await readdir(
    resolve(import.meta.dirname, "../src/space/"),
  )) {
    if (!name.endsWith(".ts") || name === "app.ts") continue;
    const source = ts.createSourceFile(
      name,
      await readFile(
        resolve(import.meta.dirname, `../src/space/${name}`),
        "utf8",
      ),
      ts.ScriptTarget.Latest,
    );
    for (const statement of source.statements) {
      if (
        !ts.isImportDeclaration(statement) &&
        !ts.isExportDeclaration(statement)
      )
        continue;
      const specifier = statement.moduleSpecifier;
      const path =
        specifier && ts.isStringLiteral(specifier) ? specifier.text : undefined;
      if (path?.startsWith("./"))
        assert.ok(
          [
            "./types.js",
            "./contracts.js",
            "./model.js",
            "./dom-types.js",
          ].includes(path),
          `${name} depends on ${path}`,
        );
    }
  }

  const emptySelection = (): SelectionState => ({
    process: null,
    connection: null,
    network: null,
    file: null,
  });
  const identity = { pid: 101, start_time_ticks: 1 };
  const node: PlacedProcess = {
    ...processInfo({ identity }),
    identity,
    parent_id: null,
    name: "writer",
    pos: { x: 0, y: 0, z: 0 },
    regions: [],
  };
  const file: RecentFile = {
    last: 0,
    id: "file",
    process_id: identity,
    label: "data.txt",
    path: "/tmp/data.txt",
    file: { device: { major: 8, minor: 1 }, inode: "9", generation: 0 },
    readBytes: 8,
    writeBytes: 12,
    readCount: 1,
    writeCount: 2,
  };
  const nodes = new Map([["101:1", node]]);

  {
    const scene = createSpaceScene();
    const input: SceneInput = {
      nodes,
      snapshot: { processes: [node], fd_relations: [] },
      network: new Map(),
      networkPositions: new Map(),
      files: new Map([["file", file]]),
      filePositions: new Map([["file", { x: 3, y: 0, z: -3 }]]),
      visible: new Set(nodes.keys()),
    };
    scene.rebuild(input, null);
    assert.deepEqual(scene.renderView().hullIds, ["101:1"]);
    assert.equal(scene.renderView().fileViews.get("file")!.pos.z, -3);
    assert.deepEqual(
      node.pos,
      { x: 0, y: 0, z: 0 },
      "scene does not convert the input coordinates in place",
    );
    const picked = scene.pickingView();
    let disposed = false;
    picked.hull!.geometry.addEventListener("dispose", () => (disposed = true));
    scene.rebuild({ ...input, visible: new Set() }, null);
    assert.equal(disposed, true, "rebuild disposes old picking geometry");
    assert.notEqual(
      scene.pickingView().hull,
      picked.hull,
      "consumers must read current picking objects",
    );
    assert.equal(
      scene.renderView().fileViews.size,
      0,
      "search filters file geometry with its owner",
    );
    scene.rebuild(input, null);
    scene.updateSelection({ ...emptySelection(), file: "file" });
    scene.refreshFiles({
      nodes,
      visible: input.visible,
      files: new Map(),
      filePositions: new Map(),
    });
    scene.updateSelection(emptySelection());
    assert.equal(
      scene.renderView().fileViews.size,
      0,
      "file refresh accepts only the needed values",
    );
    scene.dispose();
  }
  {
    const selection = new SpaceSelection();
    for (const [kind, target] of [
      ["process", "processes"],
      ["connection", "connections"],
      ["network", "networks"],
      ["file", "files"],
    ] as const) {
      const targets = {
        processes: new Set<string>(),
        connections: new Set<string>(),
        networks: new Set<string>(),
        files: new Set<string>(),
      };
      selection.choose(kind, "id");
      targets[target].add("id");
      selection.retain(targets);
      assert.equal(selection[kind], "id");
      targets[target].clear();
      selection.retain(targets);
      assert.equal(
        selection.active,
        false,
        `${kind} selection clears when its identity disappears`,
      );
    }
  }
  Object.assign(globalThis, {
    innerWidth: 800,
    innerHeight: 600,
    devicePixelRatio: 1,
  });
  const pointer = (
    canvas: HTMLCanvasElement,
    type: string,
    x = 400,
    y = 300,
  ) => {
    const event = new Event(type);
    Object.assign(event, { clientX: x, clientY: y });
    canvas.dispatchEvent(event);
  };
  {
    // Picking has no scene, store, or camera controller; only current Three.js objects.
    const canvas = document.createElement("canvas"),
      hover = document.createElement("div");
    const camera = new T.PerspectiveCamera(45, 800 / 600, 0.1, 100);
    camera.position.set(0, 0, 10);
    camera.lookAt(0, 0, 0);
    camera.updateMatrixWorld();
    const hull = new T.InstancedMesh(
      new T.BoxGeometry(2, 2, 2),
      new T.MeshBasicMaterial(),
      1,
    );
    hull.setMatrixAt(0, new T.Matrix4());
    hull.updateMatrixWorld();
    let input: PickingInput = {
      view: {
        hull,
        hullIds: ["101:1"],
        nodes: new Map([["101:1", { ...node, pos: new T.Vector3() }]]),
        objects: [],
      },
      files: new Map(),
      edgeStats: new Map(),
    };
    const events: [string, ...unknown[]][] = [];
    const unbind = bindSelection(canvas, hover, {
      camera,
      read: () => input,
      actions: {
        process: (...args) => events.push(["process", ...args]),
        connection: (id) => events.push(["connection", id]),
        network: (id) => events.push(["network", id]),
        file: (id) => events.push(["file", id]),
        clear: () => events.push(["clear"]),
      },
      hover: (state) => events.push(["hover", state]),
    });
    pointer(canvas, "pointerdown");
    pointer(canvas, "pointerup");
    pointer(canvas, "dblclick");
    assert.deepEqual(events.splice(0), [
      ["process", "101:1"],
      ["process", "101:1", true],
    ]);
    pointer(canvas, "pointermove");
    assert.match(hover.textContent!, /writer \/ 101/);
    assert.equal(hover.hidden, false);
    const marker = new T.InstancedMesh(
      new T.BoxGeometry(2, 2, 2),
      new T.MeshBasicMaterial(),
      1,
    );
    marker.setMatrixAt(0, new T.Matrix4());
    marker.updateMatrixWorld();
    marker.userData.files = [file];
    input = {
      ...input,
      view: { ...input.view, hull: null, objects: [marker] },
      files: new Map([["file", file]]),
    };
    pointer(canvas, "pointerdown");
    pointer(canvas, "pointerup");
    pointer(canvas, "pointermove");
    assert.ok(
      events.some((e) => e[0] === "file" && e[1] === "file"),
      "new geometry is read on the next click",
    );
    assert.deepEqual(events.at(-1), [
      "hover",
      { hoveredNetwork: null, hoveredFile: "file" },
    ]);
    assert.match(hover.textContent!, /READ 8 bytes · WRITE 12 bytes/);
    input = { ...input, view: { ...input.view, objects: [] } };
    pointer(canvas, "pointermove");
    assert.equal(hover.hidden, true);
    assert.deepEqual(events.at(-1), [
      "hover",
      { hoveredNetwork: null, hoveredFile: null },
    ]);
    unbind();
    const count = events.length;
    pointer(canvas, "pointerdown");
    pointer(canvas, "pointerup");
    pointer(canvas, "pointermove");
    assert.equal(events.length, count, "disposing removes pointer handlers");
    for (const mesh of [hull, marker]) {
      mesh.geometry.dispose();
      mesh.material.dispose();
    }
  }
  {
    const canvas = document.createElement("canvas");
    const camera = createSpaceCamera(canvas);
    const positions = {
      processes: [{ x: 0, y: 0, z: 0 }],
      networks: [{ x: 1000, y: 0, z: 12 }],
      files: [{ x: -100, y: 0, z: -3 }],
    };
    const density = camera.adaptWorld(positions);
    assert.equal(camera.camera.far, 4400);
    assert.equal(camera.controls.maxDistance, 2200);
    assert.equal(
      density,
      0.8 / 1100,
      "camera returns fog density for the app to wire",
    );
    camera.fit(positions);
    assert.deepEqual(camera.view().target, [450, 0, 4.5]);
    camera.focus({ x: 7, y: 8, z: 0 });
    assert.deepEqual(camera.view().target, [7, 8, 4]);
    const before = camera.view();
    camera.fit({ processes: [], networks: [], files: [] });
    assert.deepEqual(
      camera.view(),
      before,
      "empty bounds leave the camera in place",
    );
    camera.dispose();
  }

  // Record canvas drawing and substitute the GPU resource at the browser boundary.
  const context = {
    setTransform() {},
    clearRect() {},
    strokeText() {},
    fillText() {},
    beginPath() {},
    arc() {},
    fill() {},
    measureText() {
      return { width: 20 };
    },
  };
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(
    (() => context) as unknown as typeof HTMLCanvasElement.prototype.getContext,
  );
  {
    document.body.innerHTML = await readFile(
      resolve(import.meta.dirname, "../space/index.html"),
      "utf8",
    );
    const get = spaceElement;
    const actions: [string, string | null][] = [];
    const details = createSpaceDetails(
      {
        connection: (id) => actions.push(["connection", id]),
        network: (id) => actions.push(["network", id]),
      },
      get,
    );
    const data: DetailsInput = {
      nodes,
      snapshot: { processes: [], fd_relations: [] },
      network: new Map(),
      edgeStats: new Map(),
      files: new Map([["file", file]]),
    };
    details.update(data, { ...emptySelection(), process: "101:1" });
    assert.equal(get("name").textContent, "writer");
    assert.equal(
      get("inspect").getAttribute("href"),
      "/process/101?start_time_ticks=1",
    );
    details.update(data, { ...emptySelection(), file: "file" });
    assert.match(
      get("connection-facts").textContent!,
      /READ: 8 bytes.*WRITE: 12 bytes/,
    );
    const link = get("connection-endpoints").children[2];
    assert.ok(link instanceof HTMLAnchorElement);
    details.update(data, { ...emptySelection(), file: "file" });
    assert.equal(
      get("connection-endpoints").children[2],
      link,
      "live details retain the attached process link",
    );
    const edge = fdRelation({
      id: "connection",
      endpoint: fdEndpoint({ process_id: identity, fd: 3, fd_count: 1 }),
      socket: socketEndpoint(),
      label: "destination",
    });
    const group = {
      id: "network",
      endpoint: edge.endpoint,
      socket: edge.socket!,
      members: [edge],
      label: "destination",
    };
    const withNetwork = { ...data, network: new Map([["network", group]]) };
    details.update(withNetwork, { ...emptySelection(), network: "network" });
    const button = get("connection-endpoints").querySelector("button");
    assert.ok(button);
    button.click();
    assert.deepEqual(
      actions,
      [["connection", "connection"]],
      "detail buttons emit actions",
    );
    details.update(data, emptySelection());
    assert.equal(get("details").hidden, true);
  }
  {
    let pendingFrame: FrameRequestCallback | undefined;
    let frameCount = 0,
      cancelled = 0,
      renders = 0;
    vi.stubGlobal("requestAnimationFrame", (fn: FrameRequestCallback) => {
      pendingFrame = fn;
      return ++frameCount;
    });
    vi.stubGlobal("cancelAnimationFrame", () => {
      pendingFrame = undefined;
      cancelled++;
    });
    vi.stubGlobal("matchMedia", () => ({ matches: false }));
    class WebGLRenderer {
      setPixelRatio() {}
      setSize() {}
      setClearColor() {}
      dispose() {}
      render() {
        renders++;
      }
    }
    let view: RenderView = {
      nodes: new Map(),
      hullIds: [],
      networkViews: [],
      fileViews: new Map(),
      edgeViews: [],
      baseGlow: null,
      haloGlow: null,
    };
    const curve = new T.QuadraticBezierCurve3(
      new T.Vector3(),
      new T.Vector3(0, 1, 0),
      new T.Vector3(1, 1, 0),
    );
    const root = new T.Scene(),
      camera = new T.PerspectiveCamera();
    const beforeFrames: number[] = [];
    const renderer = createSpaceRenderer({
      graphics: {
        ...T,
        WebGLRenderer: WebGLRenderer as unknown as typeof T.WebGLRenderer,
      },
      canvas: document.createElement("canvas"),
      labelCanvas: document.createElement("canvas"),
      fpsLabel: document.createElement("div"),
      failure: document.createElement("div"),
      scene: root,
      camera,
      view: () => view,
      selection: () => ({
        ...emptySelection(),
        hoveredNetwork: null,
        hoveredFile: null,
      }),
      cpuGlows: new Map(),
      beforeFrame: (now) => {
        beforeFrames.push(now);
      },
    });
    view = {
      ...view,
      fileViews: new Map([["file", { file, pos: new T.Vector3(), curve }]]),
      edgeViews: [
        { e: fdRelation({ id: "connection" }), networkId: "network", curve },
      ],
    };
    renderer.activity({
      now: 1000,
      filesChanged: false,
      routes: [
        { kind: "file", id: "file", direction: 1, count: 1 },
        { kind: "connection", id: "connection", direction: -1, count: 1 },
      ],
    });
    assert.equal(renderer.fileParticles().length, 2);
    assert.equal(renderer.networkParticles().length, 2);
    renderer.retainParticles(new Set());
    assert.equal(
      renderer.networkParticles().length,
      0,
      "retention uses supplied network identities",
    );
    view = { ...view, fileViews: new Map() };
    renderer.retainParticles(new Set());
    assert.equal(
      renderer.fileParticles().length,
      0,
      "retention reads replaced geometry",
    );
    vi.spyOn(document, "hidden", "get").mockReturnValue(false);
    renderer.resume();
    renderer.resume();
    assert.equal(frameCount, 1, "resume creates only one loop");
    pendingFrame!(2000);
    assert.deepEqual(beforeFrames, [2000]);
    assert.equal(renders, 1);
    vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    pendingFrame!(3000);
    assert.deepEqual(
      beforeFrames,
      [2000],
      "hidden rendering skips app updates",
    );
    vi.spyOn(document, "hidden", "get").mockReturnValue(false);
    renderer.pause();
    assert.equal(cancelled, 1);
    assert.equal(pendingFrame, undefined);
    renderer.resume();
    pendingFrame!(4000);
    assert.deepEqual(beforeFrames, [2000, 4000]);
    const source = { ...node, pos: new T.Vector3(0, 0, 0) };
    const destination = {
      ...source,
      identity: { pid: 102, start_time_ticks: 1 },
      pos: new T.Vector3(10, 0, 0),
    };
    const glow = () =>
      new T.InstancedMesh(new T.BoxGeometry(), new T.MeshBasicMaterial(), 2);
    view = {
      ...view,
      nodes: new Map([
        ["101:1", source],
        ["102:1", destination],
      ]),
      hullIds: ["101:1", "102:1"],
      baseGlow: glow(),
      haloGlow: glow(),
    };
    const signal = {
      timestamp_ns: 123,
      src_pid: 101,
      dst_pid: 102,
      signal: 10,
      source_id: source.identity,
      destination_id: destination.identity,
    };
    const emit = (signals = [signal], now = 5000) =>
      renderer.activity({ now, filesChanged: false, routes: [], signals });
    emit();
    const flight = renderer.signalVisuals()[0];
    assert.equal(flight.label, "SIGUSR1");
    assert.deepEqual(flight.startPosition, [0, 0, 6]);
    assert.deepEqual(flight.endPosition, [10, 0, 6]);
    pendingFrame!(5100);
    const points = root.children.find(
      (o) =>
        o instanceof T.Points && (o.material as T.PointsMaterial).size === 0.8,
    ) as T.Points;
    assert.ok(points, "signal projectile has twice the particle diameter");
    assert.equal(flight.duration, 1700, "signal takes twice as long to arrive");
    assert.equal(
      points.geometry.drawRange.count,
      7,
      "one projectile has a faint six-point trail",
    );
    const x = points.geometry.attributes.position.getX(0);
    pendingFrame!(5400);
    assert.ok(
      points.geometry.attributes.position.getX(0) > x,
      "projectile advances toward destination",
    );
    const color = new T.Color();
    view.baseGlow!.getColorAt(1, color);
    assert.equal(color.r, 0, "destination waits for arrival");
    pendingFrame!(5900);
    view.baseGlow!.getColorAt(1, color);
    assert.equal(
      color.r,
      0,
      "slower projectile is still in transit after 900ms",
    );
    pendingFrame!(6750);
    view.baseGlow!.getColorAt(1, color);
    assert.ok(color.r > 0.8, "destination pulses on arrival");
    assert.equal(renderer.signalVisuals()[0].arrived, true);
    pendingFrame!(7150);
    view.baseGlow!.getColorAt(1, color);
    assert.equal(color.r, 0, "arrival pulse expires");
    assert.equal(renderer.signalVisuals().length, 0);
    emit(Array(5000).fill(signal), 8000);
    assert.equal(
      renderer.signalVisuals().length,
      256,
      "rapid activity has a global cap",
    );
    view = { ...view, hullIds: ["101:1"] };
    renderer.retainParticles(new Set());
    assert.equal(
      renderer.signalVisuals().length,
      0,
      "hidden destination clears flights",
    );
    emit();
    assert.equal(
      renderer.signalVisuals().length,
      0,
      "hidden endpoints are ignored",
    );
    view = { ...view, hullIds: ["101:1", "102:1"] };
    emit();
    renderer.clearParticles();
    assert.equal(
      renderer.signalVisuals().length,
      0,
      "SSE reset clears signals",
    );
    renderer.dispose();
    view.baseGlow!.geometry.dispose();
    view.haloGlow!.geometry.dispose();
    assert.equal(root.children.length, 0);
  }
  console.log(
    "SPACE components passed: import boundaries, plain scene inputs, current picking geometry, selection retention, camera bounds, detail links/actions, and renderer callbacks/lifecycle.",
  );
});
