import { test } from "vitest";

import assert from "node:assert/strict";
import * as T from "three";
import { createSpaceScene } from "../src/space/scene.js";
import type { SceneInput } from "../src/space/contracts.js";
import type { PlacedProcess, SelectionState } from "../src/space/types.js";
import {
  processInfo,
  fdEndpoint,
  fdRelation,
  socketEndpoint,
} from "./support/fixtures.js";

function process(
  pid: number,
  x: number,
  y: number,
  parent: number | null = null,
): PlacedProcess {
  return {
    ...processInfo({
      identity: { pid, start_time_ticks: 1 },
      parent_id: parent === null ? null : { pid: parent, start_time_ticks: 1 },
    }),
    pos: { x, y, z: 0 },
    regions: [],
  };
}
function input(processes: PlacedProcess[]): SceneInput {
  return {
    nodes: new Map(processes.map((node) => [`${node.identity.pid}:1`, node])),
    visible: new Set(processes.map((node) => `${node.identity.pid}:1`)),
    snapshot: { processes, fd_relations: [] },
    network: new Map(),
    networkPositions: new Map(),
    netlink: new Map(),
    files: new Map(),
    netlinkPositions: new Map(),
    filePositions: new Map(),
  };
}
function selection(pid: number | null): SelectionState {
  return {
    process: pid === null ? null : `${pid}:1`,
    connection: null,
    network: null,
    netlink: null,
    file: null,
  };
}
function floorMesh(scene: ReturnType<typeof createSpaceScene>) {
  const mesh = scene.root.getObjectByName("parent-lines");
  assert.ok(mesh instanceof T.InstancedMesh);
  return mesh;
}

test("floor family paths highlight every ancestor and descendant without siblings", () => {
  const scene = createSpaceScene();
  try {
    const data = input([
      process(1, 0, 0),
      process(2, -6, 10, 1),
      process(3, 6, 10, 1),
      process(4, -10, 20, 2),
      process(5, -6, 30, 4),
      process(6, 25, 0),
      process(7, 25, 10, 6),
    ]);
    scene.rebuild(data, null);
    const path = scene.parentLineVisual("1:1", "2:1")!;
    assert.deepEqual(path.points, [
      [0, 0, 0.06],
      [0, 5, 0.06],
      [-6, 5, 0.06],
      [-6, 10, 0.06],
    ]);
    assert.equal(path.width, 0.6);
    assert.equal(path.r, path.g);
    assert.equal(path.g, path.b);
    assert.equal(path.r, 0.22);
    const mesh = floorMesh(scene);
    const joints = scene.root.getObjectByName("parent-line-joints");
    assert.ok(joints instanceof T.InstancedMesh);
    assert.ok(joints.geometry instanceof T.CircleGeometry);
    assert.ok(
      !scene.pickingView().objects.includes(mesh),
      "floor ancestry is not a selectable connection",
    );
    assert.ok(!scene.pickingView().objects.includes(joints));
    const outline = joints.geometry.attributes.position;
    for (let i = 0; i < outline.count; i++)
      assert.ok(
        Math.hypot(outline.getX(i), outline.getY(i)) <= path.width / 2 + 1e-7,
        "round joins stay inside a circle with the stroke's radius",
      );
    const matrix = new T.Matrix4(),
      position = new T.Vector3(),
      rotation = new T.Quaternion(),
      scale = new T.Vector3();
    for (let i = 0; i < mesh.count; i++) {
      mesh.getMatrixAt(i, matrix);
      matrix.decompose(position, rotation, scale);
      assert.ok(Math.abs(position.z - 0.06) < 1e-7);
      assert.ok(Math.abs(scale.y - 0.6) < 1e-7);
    }
    scene.updateSelection(selection(2));
    for (const [parent, child] of [
      [1, 2],
      [2, 4],
      [4, 5],
    ])
      assert.equal(scene.parentLineVisual(`${parent}:1`, `${child}:1`)!.r, 1);
    for (const [parent, child] of [
      [1, 3],
      [6, 7],
    ])
      assert.equal(
        scene.parentLineVisual(`${parent}:1`, `${child}:1`)!.r,
        0.22,
      );
    const trunkIndices = Array.from({ length: mesh.count }, (_, i) => i).filter(
      (i) => {
        mesh.getMatrixAt(i, matrix);
        position.setFromMatrixPosition(matrix);
        return position.x === 0 && position.y === 2.5;
      },
    );
    assert.equal(
      trunkIndices.length,
      1,
      "siblings share one trunk instead of overlapping meshes",
    );
    const color = new T.Color();
    mesh.getColorAt(trunkIndices[0], color);
    assert.deepEqual(
      color.toArray(),
      [1, 1, 1],
      "shared trunk uses the active branch's color",
    );
    const jointIndices = (x: number, y: number) =>
      Array.from({ length: joints.count }, (_, i) => i).filter((i) => {
        joints.getMatrixAt(i, matrix);
        position.setFromMatrixPosition(matrix);
        assert.ok(Math.abs(position.z - 0.06) < 1e-7);
        return position.x === x && position.y === y;
      });
    for (const [x, y] of path.points)
      assert.equal(
        jointIndices(x, y).length,
        1,
        "each bend and endpoint has one round join",
      );
    joints.getColorAt(jointIndices(0, 5)[0], color);
    assert.deepEqual(
      color.toArray(),
      [1, 1, 1],
      "shared round join follows the active branch",
    );
    joints.getColorAt(jointIndices(6, 5)[0], color);
    assert.ok(
      Math.abs(color.r - 0.22) < 1e-7,
      "sibling's rounded bend stays muted",
    );
    scene.updateSelection(selection(5));
    assert.equal(scene.parentLineVisual("1:1", "2:1")!.r, 1);
    assert.equal(scene.parentLineVisual("2:1", "4:1")!.r, 1);
    scene.updateSelection(selection(null));
    for (let i = 0; i < mesh.count; i++) {
      mesh.getColorAt(i, color);
      assert.ok(Math.abs(color.r - 0.22) < 1e-7);
      assert.equal(color.r, color.g);
      assert.equal(color.g, color.b);
    }
    for (let i = 0; i < joints.count; i++) {
      joints.getColorAt(i, color);
      assert.ok(
        Math.abs(color.r - 0.22) < 1e-7,
        "clearing selection also resets round joins",
      );
    }
    let disposed = false;
    let jointsDisposed = false;
    mesh.geometry.addEventListener("dispose", () => {
      disposed = true;
    });
    joints.geometry.addEventListener("dispose", () => {
      jointsDisposed = true;
    });
    scene.rebuild({ ...data, visible: new Set(["1:1", "4:1", "5:1"]) }, null);
    scene.updateSelection(selection(1));
    assert.equal(disposed, true);
    assert.equal(jointsDisposed, true);
    assert.equal(scene.parentLineVisual("1:1", "2:1"), null);
    assert.equal(scene.parentLineVisual("2:1", "4:1"), null);
    assert.equal(
      scene.parentLineVisual("4:1", "5:1")!.r,
      1,
      "hidden generations do not interrupt descendant traversal",
    );
  } finally {
    scene.dispose();
  }
});

test("family paths handle missing parents, self parents, cycles and changed parents", () => {
  const scene = createSpaceScene();
  try {
    const data = input([
      process(1, 0, 0, 2),
      process(2, 6, 10, 1),
      process(3, 12, 20, 99),
      process(4, 18, 20, 4),
    ]);
    scene.rebuild(data, null);
    scene.updateSelection(selection(1));
    assert.equal(scene.parentLineVisual("1:1", "2:1")!.r, 1);
    assert.equal(scene.parentLineVisual("2:1", "1:1")!.r, 1);
    assert.equal(scene.parentLineVisual("99:1", "3:1"), null);
    assert.equal(scene.parentLineVisual("4:1", "4:1"), null);
    scene.rebuild(
      input([process(1, 0, 0), process(2, 6, 10), process(3, 12, 20, 2)]),
      null,
    );
    scene.updateSelection(selection(1));
    assert.equal(scene.parentLineVisual("1:1", "2:1"), null);
    assert.equal(scene.parentLineVisual("2:1", "3:1")!.r, 0.22);
    scene.rebuild(input([]), null);
    scene.updateSelection(selection(null));
    assert.equal(floorMesh(scene).count, 0);
  } finally {
    scene.dispose();
  }
});

test("partially shared trunks only brighten the sections on the selected route", () => {
  const scene = createSpaceScene();
  try {
    scene.rebuild(
      input([process(1, 0, 0), process(2, -6, 10, 1), process(3, 6, 20, 1)]),
      null,
    );
    scene.updateSelection(selection(2));
    const mesh = floorMesh(scene),
      matrix = new T.Matrix4(),
      position = new T.Vector3(),
      color = new T.Color();
    const trunks = new Map<number, number>();
    for (let i = 0; i < mesh.count; i++) {
      mesh.getMatrixAt(i, matrix);
      position.setFromMatrixPosition(matrix);
      if (position.x !== 0) continue;
      mesh.getColorAt(i, color);
      trunks.set(position.y, color.r);
    }
    assert.equal(trunks.get(2.5), 1, "shared part of the trunk is selected");
    assert.ok(
      Math.abs(trunks.get(7.5)! - 0.22) < 1e-7,
      "continuation towards the sibling stays muted",
    );
  } finally {
    scene.dispose();
  }
});

test("process sockets attach at mid-height, network connections at the roof and pipes low", () => {
  const scene = createSpaceScene();
  try {
    const data = input([process(1, 0, 0), process(2, 6, 10)]);
    const socket = (pid: number, fd: number) =>
      fdEndpoint({
        process_id: { pid, start_time_ticks: 1 },
        fd,
        kind: "socket",
        resource: {
          kind: "socket",
          device: { major: 0, minor: 1 },
          inode: "10",
        },
      });
    const connected = fdRelation({
      id: "socket",
      endpoint: socket(1, 4),
      peer: socket(2, 9),
    });
    const candidate = { ...connected, id: "candidate", candidate: true };
    const shared = { ...connected, id: "shared", shared: true };
    const dangling = { ...connected, id: "dangling", peer: null };
    const pipe = fdRelation({
      endpoint: fdEndpoint(),
      peer: fdEndpoint({ process_id: { pid: 2, start_time_ticks: 1 } }),
    });
    data.snapshot.fd_relations = [connected, candidate, shared, dangling, pipe];
    scene.rebuild(data, null);
    for (const view of scene
      .renderView()
      .edgeViews.filter((view) => view.e.id !== "pipe")) {
      for (const point of view.curve.getPoints(24))
        assert.ok(
          point.z >= 3.3 && point.z <= 4.7,
          `${view.e.id} stays at block mid-height`,
        );
    }
    const pipeCurve = scene
      .renderView()
      .edgeViews.find((view) => view.e.id === "pipe")!.curve;
    assert.ok(pipeCurve instanceof T.CubicBezierCurve3);
    assert.equal(pipeCurve.getPoint(0).z, 1.4);
    assert.ok(pipeCurve.getPoints(24).every((point) => point.z < 2.3));
    assert.ok(pipeCurve.getTangent(0).y > 0.999);
    assert.ok(pipeCurve.getTangent(1).y > 0.999);
    scene.updateSelection({ ...selection(null), connection: "socket" });
    const selectedLine = scene.root.getObjectByName(
      "selected-connection",
    ) as T.LineSegments;
    const positions = selectedLine.geometry.attributes.position;
    assert.ok(selectedLine.visible);
    for (let i = 0; i < positions.count; i++)
      assert.ok(positions.getZ(i) > 3.3);

    const external = fdRelation({
      id: "external",
      endpoint: socket(1, 5),
      socket: socketEndpoint({ network_peer: true }),
    });
    data.snapshot.fd_relations.push(external);
    data.network = new Map([
      [
        "network",
        {
          id: "network",
          endpoint: external.endpoint,
          socket: external.socket!,
          members: [external],
          label: "remote",
        },
      ],
    ]);
    data.networkPositions = new Map([["network", { x: 3, y: -3, z: 12 }]]);
    scene.rebuild(data, null);
    const network = scene.renderView().networkViews[0];
    const owner = data.nodes.get("1:1")!;
    assert.deepEqual(network.curve.v0.toArray(), [
      owner.pos.x,
      owner.pos.y,
      8.16,
    ]);
    assert.deepEqual(network.pos.toArray(), [3, -3, 12]);
    assert.equal(network.curve.getPoint(1).z, 12);
    assert.ok(network.curve.getPoints(24).every((point) => point.z >= 8));
    assert.equal(
      scene.renderView().edgeViews.find((view) => view.e.id === "external")!
        .curve,
      network.curve,
    );
  } finally {
    scene.dispose();
  }
});

test("socket ports are small plain cylinders attached to hulls and cables", () => {
  const scene = createSpaceScene();
  try {
    const data = input([process(1, 0, 0), process(2, 10, 5)]);
    data.snapshot.fd_relations = Array.from({ length: 12 }, (_, fd) => {
      const endpoint = (pid: number) =>
        fdEndpoint({
          process_id: { pid, start_time_ticks: 1 },
          fd,
          kind: "socket",
          resource: {
            kind: "socket",
            device: { major: 0, minor: 1 },
            inode: String(fd),
          },
        });
      return fdRelation({
        id: `socket-${fd}`,
        endpoint: endpoint(1),
        peer: endpoint(2),
      });
    });
    // Shared ownership and candidate evidence reuse the same physical ports.
    data.snapshot.fd_relations.push({
      ...data.snapshot.fd_relations[0],
      id: "shared",
      shared: true,
    });
    scene.rebuild(data, null);
    const stems = scene.root.getObjectByName("ipc-port-stems");
    const group = scene.root.getObjectByName("ipc-ports")!;
    assert.ok(stems instanceof T.InstancedMesh);
    assert.ok(stems.geometry instanceof T.CylinderGeometry);
    assert.equal(stems.count, 24);
    assert.equal(
      group.children.length,
      1,
      "all ports share one draw call without separate rings or discs",
    );
    assert.equal(
      stems.geometry.parameters.openEnded,
      false,
      "ports are solid cylinders",
    );
    assert.ok(stems.geometry.parameters.radiusTop <= 0.06);
    assert.ok(stems.geometry.parameters.height <= 0.16);
    assert.ok(
      !scene.pickingView().objects.includes(group),
      "decorations do not add picking work or steal cable hits",
    );
    const matrix = new T.Matrix4();
    const positions: T.Vector3[] = [];
    for (let i = 0; i < stems.count; i++) {
      stems.getMatrixAt(i, matrix);
      const stem = new T.Vector3().setFromMatrixPosition(matrix);
      const normal = new T.Vector3(0, 0, 1).transformDirection(matrix);
      const position = stem.clone().addScaledVector(normal, 0.08);
      assert.ok(Math.abs(normal.x) > 0.999);
      assert.ok(
        Math.abs(
          stem.clone().addScaledVector(normal, -0.08).x -
            (normal.x > 0 ? 1.1 : 8.9),
        ) < 1e-6,
        "cylinder base meets hull face",
      );
      assert.ok(
        scene
          .renderView()
          .edgeViews.some(
            (view) =>
              view.curve.getPoint(0).distanceTo(position) < 1e-6 ||
              view.curve.getPoint(1).distanceTo(position) < 1e-6,
          ),
      );
      assert.ok(
        positions.every((other) => other.distanceTo(position) > 0.109),
        "neighboring FDs have distinct ports",
      );
      positions.push(position);
    }
    scene.updateSelection({ ...selection(null), connection: "socket-0" });
    const highlighted = new T.Color();
    stems.getColorAt(0, highlighted);
    assert.ok(highlighted.r > 0.5, "selected cables highlight their ports");
    stems.getColorAt(2, highlighted);
    assert.ok(highlighted.r < 0.2, "other ports retain cyan styling");
    let stemsDisposed = false,
      instancesDisposed = 0;
    stems.addEventListener("dispose", () => {
      instancesDisposed++;
    });
    stems.geometry.addEventListener("dispose", () => {
      stemsDisposed = true;
    });
    scene.rebuild({ ...data, visible: new Set(["1:1"]) }, null);
    assert.ok(stemsDisposed, "rebuild releases port geometry");
    assert.equal(instancesDisposed, 1, "rebuild releases instance buffers too");
    assert.equal(
      (scene.root.getObjectByName("ipc-port-stems") as T.InstancedMesh).count,
      0,
    );
  } finally {
    scene.dispose();
  }
});
