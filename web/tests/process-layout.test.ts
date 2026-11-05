import { test } from "vitest";

import assert from "node:assert/strict";
import { treeLayout, stableLayout } from "../src/space/process-layout.js";
import { SpaceDataStore } from "../src/space/data.js";
import { key, PROCESS_SIDE } from "../src/space/model.js";
import { processInfo, fdEndpoint, fdRelation } from "./support/fixtures.js";
import type { FdRelation, Process } from "../src/shared/api-types.js";

const process = (pid: number, parent: number | null = 1) =>
  processInfo({
    identity: { pid, start_time_ticks: 1 },
    parent_id: parent === null ? null : { pid: parent, start_time_ticks: 1 },
  });
const socket = (a: Process, b: Process): FdRelation =>
  fdRelation({
    id: `${a.identity.pid}-${b.identity.pid}`,
    endpoint: fdEndpoint({
      process_id: a.identity,
      kind: "socket",
      resource: { kind: "socket", device: { major: 0, minor: 1 }, inode: "10" },
    }),
    peer: fdEndpoint({
      process_id: b.identity,
      kind: "socket",
      resource: { kind: "socket", device: { major: 0, minor: 1 }, inode: "11" },
    }),
  });
const fixture = () => {
  const root = process(1, null),
    hub = process(2),
    leaves = Array.from({ length: 80 }, (_, i) => process(i + 3)),
    branches = [process(200), process(300)];
  const nodes = [
    root,
    hub,
    ...leaves,
    ...branches,
    ...branches.flatMap((parent) =>
      Array.from({ length: 20 }, (_, i) =>
        process(parent.identity.pid + i + 1, parent.identity.pid),
      ),
    ),
  ];
  return {
    nodes,
    hub,
    leaves,
    branches,
    relations: leaves.map((peer) => socket(hub, peer)),
  };
};

test("socket hubs move to the middle of wrapped leaves and sibling branches", () => {
  const { nodes, hub, leaves, branches, relations } = fixture();
  const original = treeLayout(nodes),
    layout = treeLayout(nodes, undefined, undefined, relations);
  const before = original.get(key(hub.identity))!,
    after = layout.get(key(hub.identity))!;
  assert.ok(Math.abs(after.x) < Math.abs(before.x) / 2);
  const totalLength = (positions: typeof layout) =>
    leaves.reduce((sum, leaf) => {
      const a = positions.get(key(hub.identity))!,
        b = positions.get(key(leaf.identity))!;
      return sum + Math.hypot(a.x - b.x, a.y - b.y);
    }, 0);
  assert.ok(totalLength(layout) < totalLength(original) * 0.8);
  const leafPositions = [hub, ...leaves].map((node) =>
    layout.get(key(node.identity))!,
  );
  const centerX =
      (Math.min(...leafPositions.map((p) => p.x)) +
        Math.max(...leafPositions.map((p) => p.x))) /
      2,
    centerY =
      (Math.min(...leafPositions.map((p) => p.y)) +
        Math.max(...leafPositions.map((p) => p.y))) /
      2;
  const distance = (p: typeof after) =>
    Math.hypot(p.x - centerX, p.y - centerY);
  assert.equal(distance(after), Math.min(...leafPositions.map(distance)));
  const minLeafX = Math.min(...leafPositions.map((p) => p.x)),
    maxLeafX = Math.max(...leafPositions.map((p) => p.x));
  for (const branch of branches) {
    const xs = nodes
      .filter((n) => n === branch || n.parent_id?.pid === branch.identity.pid)
      .map((n) => layout.get(key(n.identity))!.x);
    assert.ok(Math.max(...xs) < minLeafX || Math.min(...xs) > maxLeafX);
  }
  const places = [...layout];
  for (let i = 0; i < places.length; i++) {
    const [id, place] = places[i];
    assert.equal(place.parent, original.get(id)!.parent);
    assert.equal(place.depth, original.get(id)!.depth);
    for (const [, other] of places.slice(i + 1))
      assert.ok(
        Math.abs(place.x - other.x) >= PROCESS_SIDE ||
          Math.abs(place.y - other.y) >= PROCESS_SIDE,
      );
  }
});

test("a socket hub with descendants moves together with its entire subtree", () => {
  const branches = [process(2), process(100), process(200)];
  const descendants = branches.flatMap((parent) =>
    Array.from({ length: 9 }, (_, i) =>
      process(parent.identity.pid + i + 1, parent.identity.pid),
    ),
  );
  const nodes = [process(1, null), ...branches, ...descendants];
  const hub = branches[0];
  const relations = descendants
    .filter((node) => node.parent_id!.pid !== hub.identity.pid)
    .map((node) => socket(hub, node));
  const original = treeLayout(nodes),
    layout = treeLayout(nodes, undefined, undefined, relations);
  const before = original.get(key(hub.identity))!,
    after = layout.get(key(hub.identity))!;
  assert.ok(Math.abs(after.x) < Math.abs(before.x) / 2);
  for (const node of descendants.filter(
    (node) => node.parent_id!.pid === hub.identity.pid,
  )) {
    const old = original.get(key(node.identity))!,
      placed = layout.get(key(node.identity))!;
    assert.ok(Math.abs(placed.x - after.x - (old.x - before.x)) < 1e-9);
    assert.equal(placed.y - after.y, old.y - before.y);
    assert.equal(placed.parent, key(hub.identity));
  }
});

test("socket priorities count distinct identities independent of FD order and direction", () => {
  const { nodes, hub, leaves, relations } = fixture();
  const decoy = socket(leaves[0], leaves[1]);
  const distinct = [...relations, decoy];
  const layout = treeLayout(nodes, undefined, undefined, distinct);
  const reversed = distinct
    .map((r) => ({ ...r, endpoint: r.peer!, peer: r.endpoint }))
    .reverse();
  const duplicates = Array.from({ length: 200 }, (_, i) => ({
    ...decoy,
    id: `duplicate-${i}`,
  }));
  assert.deepEqual(
    treeLayout([...nodes].reverse(), undefined, undefined, [
      ...reversed,
      ...duplicates,
    ]),
    layout,
  );
  assert.deepEqual(
    stableLayout(nodes, new Map(), undefined, undefined, distinct),
    layout,
  );
  assert.ok(layout.has(key(hub.identity)));
});

test("shared, candidate, external, pipe, self and stale socket relations do not prioritize", () => {
  const { nodes, hub, relations } = fixture();
  const ignored = relations.flatMap((r) => [
    { ...r, shared: true },
    { ...r, candidate: true },
    { ...r, peer: null },
    {
      ...r,
      endpoint: {
        ...r.endpoint,
        resource: { ...r.endpoint.resource, kind: "pipe" as const },
      },
    },
    {
      ...r,
      peer: {
        ...r.peer!,
        process_id: { pid: r.peer!.process_id.pid, start_time_ticks: 2 },
      },
    },
    { ...r, peer: { ...r.peer!, process_id: process(9999).identity } },
    socket(hub, hub),
  ]);
  assert.deepEqual(
    treeLayout(nodes, undefined, undefined, ignored),
    treeLayout(nodes),
  );
  assert.deepEqual(
    treeLayout(nodes, undefined, undefined, [relations[0]]),
    treeLayout(nodes),
  );
});

test("snapshot updates keep positions while Rearrange applies current socket priorities", () => {
  const { nodes, hub, relations } = fixture();
  const store = new SpaceDataStore();
  store.replaceSnapshot({ processes: nodes, fd_relations: relations });
  const positions = () =>
    new Map([...store.nodes].map(([id, node]) => [id, { ...node.pos }]));
  const initial = positions();
  const expected = treeLayout(nodes, undefined, undefined, relations).get(
    key(hub.identity),
  )!;
  assert.deepEqual(initial.get(key(hub.identity)), {
    x: expected.x,
    y: expected.y,
    z: 0,
  });
  store.replaceSnapshot({
    processes: [...nodes, process(9999)],
    fd_relations: [],
  });
  for (const [id, position] of initial)
    assert.deepEqual(store.nodes.get(id)!.pos, position);
  store.replaceSnapshot({ processes: nodes, fd_relations: [] }, true);
  const plain = treeLayout(nodes).get(key(hub.identity))!;
  assert.deepEqual(store.nodes.get(key(hub.identity))!.pos, {
    x: plain.x,
    y: plain.y,
    z: 0,
  });
  store.replaceSnapshot({ processes: nodes, fd_relations: relations }, true);
  assert.deepEqual(positions(), initial);
});
