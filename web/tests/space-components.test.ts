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

test("connection detail cards stay ordered when collected endpoints reverse", async () => {
  const { createSpaceDetails } = await import("../src/space/details.js");
  const { key } = await import("../src/space/model.js");
  document.body.innerHTML = await readFile(
    resolve(import.meta.dirname, "../space/index.html"),
    "utf8",
  );
  const details = createSpaceDetails({ connection() {}, network() {} });
  const selection: SelectionState = {
    process: null,
    connection: "connection",
    network: null,
    netlink: null,
    file: null,
  };
  for (const [low, high] of [
    [
      { pid: 2, ticks: 90, fd: 10 },
      { pid: 10, ticks: 1, fd: 2 },
    ],
    [
      { pid: 10, ticks: 2, fd: 10 },
      { pid: 10, ticks: 10, fd: 2 },
    ],
    [
      { pid: 10, ticks: 2, fd: 2 },
      { pid: 10, ticks: 2, fd: 10 },
    ],
  ]) {
    const endpoints = [low, high].map(({ pid, ticks, fd }) =>
      fdEndpoint({ process_id: { pid, start_time_ticks: ticks }, fd }),
    );
    const nodes = new Map<string, PlacedProcess>();
    for (const { process_id } of endpoints)
      nodes.set(key(process_id), {
        ...processInfo({
          identity: process_id,
          name: `process-${key(process_id)}`,
        }),
        pos: { x: 0, y: 0, z: 0 },
        regions: [],
      });
    for (const style of [{}, { candidate: true }, { shared: true }]) {
      const edge = fdRelation({
        id: "connection",
        endpoint: endpoints[0],
        peer: endpoints[1],
        label: "UNIX STREAM",
        socket: socketEndpoint({
          protocol: { kind: "unix", socket_type: { kind: "stream" } },
          path: "/run/test.sock",
        }),
        ...style,
      });
      let cards: Element[] | undefined;
      for (const reversed of [true, false, true]) {
        const current = reversed
          ? { ...edge, endpoint: edge.peer!, peer: edge.endpoint }
          : edge;
        details.update(
          {
            nodes,
            snapshot: { processes: [], fd_relations: [current] },
            network: new Map(),
            edgeStats: new Map(),
            files: new Map(),
            netlink: new Map(),
          },
          selection,
        );
        const rendered = [...spaceElement("connection-endpoints").children];
        assert.equal(
          rendered.length,
          2,
          "only the endpoint cards are rendered",
        );
        rendered.forEach((card, index) => {
          const { process_id, fd } = endpoints[index];
          assert.equal(
            card.querySelector("a")?.getAttribute("href"),
            `/process/${process_id.pid}?start_time_ticks=${process_id.start_time_ticks}`,
          );
          assert.match(card.textContent!, new RegExp(`FD ${fd} ·`));
          if (cards)
            assert.equal(card, cards[index], "updates keep each card attached");
        });
        cards = rendered;
        assert.equal(
          current.endpoint,
          reversed ? endpoints[1] : endpoints[0],
          "display ordering does not mutate the collected relation",
        );
      }
    }
  }
});

test("socket attachment faces survive endpoint reversal across rebuilds", async () => {
  const { createSpaceScene } = await import("../src/space/scene.js");
  const { key } = await import("../src/space/model.js");
  const pairs = [
    [
      { pid: 2, start_time_ticks: 90, fd: 10 },
      { pid: 10, start_time_ticks: 1, fd: 2 },
    ],
    [
      { pid: 10, start_time_ticks: 2, fd: 10 },
      { pid: 10, start_time_ticks: 10, fd: 2 },
    ],
    [
      { pid: 10, start_time_ticks: 2, fd: 2 },
      { pid: 10, start_time_ticks: 2, fd: 10 },
    ],
  ];
  for (const [low, high] of pairs) {
    const endpoints = [low, high].map(({ fd, ...process_id }) =>
      fdEndpoint({
        process_id,
        fd,
        kind: "socket",
        resource: {
          kind: "socket",
          device: { major: 0, minor: 1 },
          inode: String(fd),
        },
      }),
    );
    const nodes = new Map<string, PlacedProcess>();
    for (const endpoint of endpoints) {
      const identity = endpoint.process_id;
      if (!nodes.has(key(identity)))
        nodes.set(key(identity), {
          ...processInfo({ identity }),
          pos: { x: nodes.size * 10, y: nodes.size * 5, z: 0 },
          regions: [],
        });
    }
    const scene = createSpaceScene();
    const input: SceneInput = {
      nodes,
      snapshot: { processes: [...nodes.values()], fd_relations: [] },
      network: new Map(),
      networkPositions: new Map(),
      netlink: new Map(),
      files: new Map(),
      netlinkPositions: new Map(),
      filePositions: new Map(),
      visible: new Set(nodes.keys()),
    };
    const expected = endpoints.map((endpoint, index) => {
      const pos = nodes.get(key(endpoint.process_id))!.pos;
      return new T.Vector3(
        pos.x +
          (index === 0 ? 1.26 : -1.26) *
            (nodes.size === 1 && String(low.fd) > String(high.fd) ? -1 : 1),
        pos.y,
        4,
      );
    });
    const colors = new Set<string>();
    for (const style of [{}, { candidate: true }, { shared: true }]) {
      const relation = fdRelation({
        id: "socket",
        endpoint: endpoints[0],
        peer: endpoints[1],
        socket: socketEndpoint(),
        ...style,
      });
      let forward: T.Curve<T.Vector3> | undefined;
      let segments: number[][] | undefined;
      // Start with the reverse orientation too, so a cache cannot mask the bug.
      for (const reversed of [true, false, true]) {
        const edge = reversed
          ? { ...relation, endpoint: relation.peer!, peer: relation.endpoint }
          : relation;
        scene.rebuild(
          { ...input, snapshot: { ...input.snapshot, fd_relations: [edge] } },
          null,
        );
        const view = scene.renderView().edgeViews[0];
        assert.equal(
          view.e,
          edge,
          "rendering preserves the relation's logical orientation",
        );
        assert.deepEqual(
          view.curve.getPoint(0).toArray(),
          expected[reversed ? 1 : 0].toArray(),
        );
        assert.deepEqual(
          view.curve.getPoint(1).toArray(),
          expected[reversed ? 0 : 1].toArray(),
        );
        if (!reversed) forward = view.curve;
        else if (forward)
          for (const t of [0, 0.25, 0.5, 0.75, 1]) {
            assert.ok(
              forward.getPoint(t).distanceTo(view.curve.getPoint(1 - t)) < 1e-9,
              "reversing the relation preserves the path and reverses its animation direction",
            );
          }
        const relationLine = scene
          .pickingView()
          .objects.find(
            (object) =>
              object instanceof T.LineSegments &&
              object.userData.fd_relations?.includes(edge),
          ) as T.LineSegments;
        const firstSegment = relationLine.userData.fd_relations.indexOf(edge);
        const color = relationLine.geometry.getAttribute("color");
        colors.add(
          [
            color.getX(firstSegment * 2),
            color.getY(firstSegment * 2),
            color.getZ(firstSegment * 2),
          ].join(":"),
        );
        if (relation.candidate || relation.shared) {
          const line = scene
            .pickingView()
            .objects.find(
              (object) =>
                object instanceof T.LineSegments &&
                object.userData.fd_relations?.[0] === edge,
            ) as T.LineSegments;
          const positions = line.geometry.getAttribute("position");
          const samples = view.curve.getPoints(16);
          assert.equal(
            positions.count,
            (samples.length - 1) * 2,
            "shared and candidate relations draw every part of the path",
          );
          for (let i = 0; i < positions.count; i++)
            assert.ok(
              new T.Vector3()
                .fromBufferAttribute(positions, i)
                .distanceTo(samples[Math.floor(i / 2) + (i % 2)]) < 1e-6,
              "colored relations cover the full curve without dash gaps",
            );
          const current: number[][] = [];
          for (let i = 0; i < positions.count; i += 2) {
            const ends = [
              new T.Vector3().fromBufferAttribute(positions, i).toArray(),
              new T.Vector3().fromBufferAttribute(positions, i + 1).toArray(),
            ];
            ends.sort((a, b) => a[0] - b[0] || a[1] - b[1] || a[2] - b[2]);
            current.push(ends.flat());
          }
          current.sort(
            (a, b) =>
              a[0] - b[0] ||
              a[1] - b[1] ||
              a[2] - b[2] ||
              a[3] - b[3] ||
              a[4] - b[4] ||
              a[5] - b[5],
          );
          if (segments)
            assert.ok(
              current.every((segment, i) =>
                segment.every(
                  (value, j) => Math.abs(value - segments![i][j]) < 1e-6,
                ),
              ),
              "the colored relation segments stay in place within GPU coordinate precision",
            );
          segments = current;
        }
      }
    }
    assert.equal(
      colors.size,
      3,
      "confirmed, candidate, and shared relations have distinct colors",
    );
    scene.dispose();
  }
});

test("socket paths clear endpoint hulls and loop outside self-connections between opposite faces", async () => {
  const { createSpaceScene } = await import("../src/space/scene.js");
  const { PROCESS_SIDE, key } = await import("../src/space/model.js");
  const owner: PlacedProcess = {
    ...processInfo({ identity: { pid: 10, start_time_ticks: 1 } }),
    pos: { x: 3, y: -7, z: 0 },
    regions: [],
  };
  const peer: PlacedProcess = {
    ...processInfo({ identity: { pid: 2, start_time_ticks: 1 } }),
    pos: { x: 0, y: 0, z: 0 },
    regions: [],
  };
  const endpoint = (node: PlacedProcess, fd: number) =>
    fdEndpoint({
      process_id: node.identity,
      fd,
      kind: "socket",
      resource: {
        kind: "socket",
        device: { major: 0, minor: 1 },
        inode: String(fd),
      },
    });
  const nodes = new Map([owner, peer].map((n) => [key(n.identity), n]));
  const input: SceneInput = {
    nodes,
    snapshot: { processes: [...nodes.values()], fd_relations: [] },
    network: new Map(),
    networkPositions: new Map(),
    netlink: new Map(),
    files: new Map(),
    netlinkPositions: new Map(),
    filePositions: new Map(),
    visible: new Set(nodes.keys()),
  };
  const scene = createSpaceScene();
  // Reuse the scene to exercise attachment updates after layout changes.
  const placements = [
    [10, 0, 1, 0],
    [-10, 0, -1, 0],
    [0, 10, 0, 1],
    [0, -10, 0, -1],
    [10, 5, 1, 0],
    [-10, -5, -1, 0],
    [5, 10, 0, 1],
    [-5, -10, 0, -1],
    [10, 10, 1, 0],
    [-10, 10, -1, 0],
    [10, -10, 1, 0],
    [-10, -10, -1, 0],
    [0, 0, 1, 0],
  ];
  for (const [dx, dy, nx, ny] of placements) {
    peer.pos = { x: owner.pos.x + dx, y: owner.pos.y + dy, z: 0 };
    const self = dx === 0 && dy === 0,
      destination = self ? owner : peer;
    for (const peerFd of [4, 15]) {
      const relation = fdRelation({
        endpoint: endpoint(owner, 3),
        peer: endpoint(destination, peerFd),
        socket: socketEndpoint(),
      });
      const faceSign = self && "3" > String(peerFd) ? -1 : 1;
      const normal = new T.Vector3(nx * faceSign, ny * faceSign, 0);
      const expectedStart = new T.Vector3(
        owner.pos.x,
        owner.pos.y,
        4,
      ).addScaledVector(normal, PROCESS_SIDE / 2 + 0.16);
      const expectedEnd = new T.Vector3(
        destination.pos.x,
        destination.pos.y,
        4,
      ).addScaledVector(normal, -(PROCESS_SIDE / 2 + 0.16));
      for (const style of [{}, { candidate: true }, { shared: true }]) {
        let forward: T.Curve<T.Vector3> | undefined;
        for (const reversed of [false, true]) {
          const edge = {
            ...relation,
            ...style,
            ...(reversed
              ? { endpoint: relation.peer!, peer: relation.endpoint }
              : {}),
          };
          scene.rebuild(
            { ...input, snapshot: { ...input.snapshot, fd_relations: [edge] } },
            null,
          );
          const curve = scene.renderView().edgeViews[0].curve;
          assert.ok(
            curve
              .getPoint(0)
              .distanceTo(reversed ? expectedEnd : expectedStart) < 1e-9,
          );
          assert.ok(
            curve
              .getPoint(1)
              .distanceTo(reversed ? expectedStart : expectedEnd) < 1e-9,
          );
          if (!reversed) forward = curve;
          else
            for (const t of [0, 0.25, 0.5, 0.75, 1])
              assert.ok(
                curve.getPoint(t).distanceTo(forward!.getPoint(1 - t)) < 1e-9,
              );
          const outward = normal.clone().multiplyScalar(reversed ? -1 : 1);
          assert.ok(curve.getTangent(0).dot(outward) > 0.999);
          assert.ok(curve.getTangent(1).dot(outward) > 0.999);
          if (self) {
            const middle = curve.getPoint(0.5);
            assert.ok(middle.y > owner.pos.y + PROCESS_SIDE / 2);
            assert.ok(
              curve.getPoint(0).distanceTo(curve.getPoint(1)) > PROCESS_SIDE,
            );
          }
          const line = scene
            .pickingView()
            .objects.find(
              (object) =>
                object instanceof T.LineSegments &&
                object.userData.fd_relations?.includes(edge),
            ) as T.LineSegments;
          const positions = line.geometry.getAttribute("position");
          for (let i = 0; i < line.userData.fd_relations.length; i++) {
            if (line.userData.fd_relations[i] !== edge) continue;
            const start = new T.Vector3().fromBufferAttribute(positions, 2 * i),
              end = new T.Vector3().fromBufferAttribute(positions, 2 * i + 1),
              ray = new T.Ray(start, end.clone().sub(start).normalize());
            for (const node of [owner, destination]) {
              const half = PROCESS_SIDE / 2,
                box = new T.Box3(
                  new T.Vector3(node.pos.x - half, node.pos.y - half, 0),
                  new T.Vector3(node.pos.x + half, node.pos.y + half, 8),
                ),
                hit = ray.intersectBox(box, new T.Vector3());
              assert.ok(
                !box.containsPoint(start) &&
                  (!hit || hit.distanceTo(start) > start.distanceTo(end)),
                `socket segment enters endpoint hull at layout (${dx}, ${dy})`,
              );
            }
          }
        }
      }
    }
  }
  scene.dispose();
});

test("pipes and dangling sockets terminate at face-aligned cylinders", async () => {
  const { createSpaceScene } = await import("../src/space/scene.js");
  const node: PlacedProcess = {
    ...processInfo({ identity: { pid: 10, start_time_ticks: 1 } }),
    pos: { x: 0, y: 0, z: 0 },
    regions: [],
  };
  const peer: PlacedProcess = {
    ...processInfo({ identity: { pid: 2, start_time_ticks: 1 } }),
    pos: { x: 10, y: 5, z: 0 },
    regions: [],
  };
  const pipe = fdRelation({
    endpoint: fdEndpoint({ process_id: node.identity, fd: 3 }),
    peer: fdEndpoint({ process_id: peer.identity, fd: 4 }),
  });
  const external = fdRelation({
    id: "external",
    endpoint: fdEndpoint({
      process_id: node.identity,
      fd: 5,
      kind: "socket",
      resource: { kind: "socket", device: { major: 0, minor: 1 }, inode: "5" },
    }),
    socket: socketEndpoint(),
  });
  const scene = createSpaceScene();
  scene.rebuild(
    {
      nodes: new Map([
        ["10:1", node],
        ["2:1", peer],
      ]),
      snapshot: { processes: [node, peer], fd_relations: [pipe, external] },
      network: new Map(),
      networkPositions: new Map(),
      netlink: new Map(),
      files: new Map(),
      netlinkPositions: new Map(),
      filePositions: new Map(),
      visible: new Set(["10:1", "2:1"]),
    },
    null,
  );
  const [pipeView, externalView] = scene.renderView().edgeViews;
  assert.deepEqual(pipeView.curve.getPoint(0).toArray(), [1.26, 0, 1.4]);
  assert.deepEqual(pipeView.curve.getPoint(1).toArray(), [10 - 1.26, 5, 1.4]);
  assert.ok(pipeView.curve instanceof T.CubicBezierCurve3);
  assert.ok(pipeView.curve.getTangent(0).x > 0.999);
  assert.ok(pipeView.curve.getTangent(1).x > 0.999);
  assert.deepEqual(externalView.curve.getPoint(0).toArray(), [1.26, 0, 4]);
  assert.deepEqual(externalView.curve.getPoint(1).toArray(), [
    1.26 + 3.5,
    -2.5,
    4,
  ]);
  scene.dispose();
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
            "./clock.js",
            "./contracts.js",
            "./model.js",
            "./netlink.js",
            "./dom-types.js",
          ].includes(path) ||
            (name === "data.ts" && path === "./process-layout.js") ||
            (name === "session-controls.ts" && path === "./recording.js") ||
            (name === "scene.ts" && path === "./ipc-ports.js"),
          `${name} depends on ${path}`,
        );
    }
  }

  const emptySelection = (): SelectionState => ({
    process: null,
    connection: null,
    network: null,
    netlink: null,
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
      netlink: new Map(),
      files: new Map([["file", file]]),
      netlinkPositions: new Map(),
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
      netlink: new Map(),
      files: new Map(),
      netlinkPositions: new Map(),
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
      ["netlink", "netlink"],
    ] as const) {
      const targets = {
        processes: new Set<string>(),
        connections: new Set<string>(),
        networks: new Set<string>(),
        netlink: new Set<string>(),
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
      netlink: new Map(),
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
        netlink: (id) => events.push(["netlink", id]),
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
      netlink: new Map(),
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
      { hoveredNetwork: null, hoveredFile: "file", hoveredNetlink: null },
    ]);
    assert.match(hover.textContent!, /READ 8 bytes · WRITE 12 bytes/);
    input = { ...input, view: { ...input.view, objects: [] } };
    pointer(canvas, "pointermove");
    assert.equal(hover.hidden, true);
    assert.deepEqual(events.at(-1), [
      "hover",
      { hoveredNetwork: null, hoveredFile: null, hoveredNetlink: null },
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
      netlink: [],
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
    camera.fit({ processes: [], networks: [], files: [], netlink: [] });
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
      netlink: new Map(),
      files: new Map([["file", file]]),
    };
    const commandLine = ["/usr/bin/writer", "<b>literal</b>", "x".repeat(1000)];
    nodes.get("101:1")!.command_line = commandLine;
    details.update(data, { ...emptySelection(), process: "101:1" });
    assert.equal(get("name").textContent, "writer");
    assert.equal(get("cmdline").textContent, commandLine.join(" "));
    assert.equal(get("cmdline").children.length, 0);
    assert.ok(get("name") instanceof HTMLAnchorElement);
    assert.equal(
      get("name").getAttribute("href"),
      "/process/101?start_time_ticks=1",
    );
    for (const command_line of [null, [], ["/usr/bin/writer", "--updated"]]) {
      nodes.get("101:1")!.command_line = command_line;
      details.update(data, { ...emptySelection(), process: "101:1" });
      assert.equal(
        get("cmdline").textContent,
        command_line?.join(" ") || "N/A",
      );
    }
    details.update(data, { ...emptySelection(), file: "file" });
    assert.match(
      get("connection-facts").textContent!,
      /READ: 8 bytes.*WRITE: 12 bytes/,
    );
    const link = get("connection-endpoints").querySelector("a");
    assert.ok(link instanceof HTMLAnchorElement);
    assert.equal(link.textContent, "writer");
    assert.equal(link.getAttribute("href"), "/process/101?start_time_ticks=1");
    details.update(data, { ...emptySelection(), file: "file" });
    assert.equal(
      get("connection-endpoints").querySelector("a"),
      link,
      "live details retain the attached process link",
    );
    const edge = fdRelation({
      id: "connection",
      endpoint: fdEndpoint({ process_id: identity, fd: 3, fd_count: 1 }),
      peer: fdEndpoint({ process_id: { pid: 101, start_time_ticks: 2 } }),
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
    const networkLink = get("connection-endpoints").querySelector("a");
    assert.ok(networkLink instanceof HTMLAnchorElement);
    assert.equal(networkLink.textContent, "writer");
    assert.equal(
      networkLink.getAttribute("href"),
      "/process/101?start_time_ticks=1",
    );
    details.update(withNetwork, { ...emptySelection(), network: "network" });
    assert.equal(
      get("connection-endpoints").querySelector("a"),
      networkLink,
      "live network details retain the attached process link",
    );
    const button = get("connection-endpoints").querySelector("button");
    assert.ok(button);
    button.click();
    assert.deepEqual(
      actions,
      [["connection", "connection"]],
      "detail buttons emit actions",
    );
    const withConnection = {
      ...data,
      snapshot: { ...data.snapshot, fd_relations: [edge] },
    };
    details.update(withConnection, {
      ...emptySelection(),
      connection: "connection",
    });
    const endpointLinks = [
      ...get("connection-endpoints").querySelectorAll("strong a"),
    ];
    assert.deepEqual(
      endpointLinks.map((a) => [a.textContent, a.getAttribute("href")]),
      [
        ["writer", "/process/101?start_time_ticks=1"],
        ["Unknown process", "/process/101?start_time_ticks=2"],
      ],
      "endpoint titles link to each observed identity, including a reused PID",
    );
    details.update(withConnection, {
      ...emptySelection(),
      connection: "connection",
    });
    assert.equal(
      get("connection-endpoints").querySelector("strong a"),
      endpointLinks[0],
      "live connection details retain the attached title link",
    );
    details.update(
      {
        ...withConnection,
        snapshot: { ...data.snapshot, fd_relations: [{ ...edge, peer: null }] },
      },
      { ...emptySelection(), connection: "connection" },
    );
    assert.equal(get("connection-endpoints").querySelectorAll("a").length, 1);
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
      netlinkViews: new Map(),
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
        hoveredNetlink: null,
      }),
      cpuGlows: new Map(),
      beforeFrame: (now) => {
        beforeFrames.push(now);
      },
    });
    view = {
      ...view,
      netlinkViews: new Map(),
      fileViews: new Map([["file", { file, pos: new T.Vector3(), curve }]]),
      edgeViews: [
        { e: fdRelation({ id: "connection" }), networkId: "network", curve },
      ],
    };
    renderer.activity({
      now: 1000,
      filesChanged: false,
      netlinkChanged: false,
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
      renderer.activity({
        now,
        filesChanged: false,
        netlinkChanged: false,
        routes: [],
        signals,
      });
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
