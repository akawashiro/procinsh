import { expect, test } from "vitest";

import { SpaceDataStore } from "../src/space/data.js";
import { isKernelNetlink } from "../src/space/netlink.js";
import { createSpaceScene } from "../src/space/scene.js";
import {
  fdEndpoint,
  fdRelation,
  processInfo,
  socketEndpoint,
  spaceActivity,
} from "./support/fixtures.js";
import type {
  FdEndpoint,
  FdRelation,
  IoActivity,
} from "../src/shared/api-types.js";

const a = { pid: 101, start_time_ticks: 1 };
const b = { pid: 102, start_time_ticks: 2 };
function endpoint(inode: string, fd = 3, process_id = a) {
  return fdEndpoint({
    process_id,
    fd,
    kind: "socket",
    resource: { kind: "socket", device: { major: 0, minor: 1 }, inode },
  });
}
function relation(
  endpoint: FdEndpoint,
  protocol = 0,
  extra: Partial<FdRelation> = {},
) {
  return fdRelation({
    id: `${endpoint.process_id.pid}:${endpoint.fd}:${endpoint.resource.inode}`,
    endpoint,
    peer: null,
    socket: socketEndpoint({
      protocol: { kind: "other", family: 16, socket_type: null, protocol },
      state: null,
    }),
    ...extra,
  });
}
function event(endpoint: FdEndpoint, write = true): IoActivity {
  return {
    process_id: endpoint.process_id,
    resource: endpoint.resource,
    write,
    bytes: 100,
    count: 1,
  };
}
function store(relations: FdRelation[]) {
  const data = new SpaceDataStore();
  data.replaceSnapshot(
    {
      processes: [processInfo({ identity: a }), processInfo({ identity: b })],
      fd_relations: relations,
    },
    false,
    0,
  );
  return data;
}

test("kernel-facing classification leaves USERSOCK and other socket families unchanged", () => {
  expect(isKernelNetlink(relation(endpoint("1"), 0).socket)).toBe(true);
  expect(isKernelNetlink(relation(endpoint("1"), 999).socket)).toBe(true);
  expect(isKernelNetlink(relation(endpoint("1"), 2).socket)).toBe(false);
  expect(isKernelNetlink(socketEndpoint())).toBe(false);
  expect(isKernelNetlink(null)).toBe(false);
});

test("snapshot sockets start at zero and remain visible without traffic or a time limit", () => {
  const fd = endpoint("1");
  const data = store([relation(fd)]);
  expect(data.netlinkSockets.entries.size).toBe(1);
  const socket = [...data.netlinkSockets.entries.values()][0];
  expect(socket).toMatchObject({
    readBytes: 0,
    writeBytes: 0,
    readCount: 0,
    writeCount: 0,
  });
  expect(data.netlinkPositions.size).toBe(1);
  expect(data.pruneFiles(60000)).toBe(false);
  expect(data.netlinkSockets.entries.get(socket.id)).toBe(socket);
  const update = data.ingestActivity(
    spaceActivity({ ipc: [event(fd), event(fd, false)] }),
    undefined,
    60010,
  );
  expect(update.netlinkChanged).toBe(false);
  expect(update.routes.map((r) => [r.kind, r.direction])).toEqual([
    ["netlink", 1],
    ["netlink", -1],
  ]);
  expect(socket).toMatchObject({
    readBytes: 100,
    writeBytes: 100,
    readCount: 1,
    writeCount: 1,
  });
  expect(socket.label).toBe("NETLINK ROUTE · FD 3");
  expect(data.edgeStats.size).toBe(0);
  data.replaceSnapshot(data.snapshot, false, 120000);
  expect(data.pruneFiles(180000)).toBe(false);
  expect(data.netlinkSockets.entries.get(socket.id)).toBe(socket);
  expect([...data.netlinkSockets.entries.keys()]).toEqual([socket.id]);
});

test("shared relations and duplicate FDs normalize owners without duplicating traffic", () => {
  const one = { ...endpoint("1"), fd_count: 2 },
    other = endpoint("1", 5, b),
    duplicate = endpoint("1", 7);
  const data = store([
    relation(one, 0, { shared: true, peer: other }),
    relation(one, 0, { id: "same-owner-again" }),
    relation(duplicate, 0, { shared: true, peer: other }),
    relation(endpoint("2", 8)),
  ]);
  const update = data.ingestActivity(
    {
      ...spaceActivity(),
      ipc: [event(one), event(other, false), event(endpoint("2", 8))],
    },
    undefined,
    10,
  );
  expect(update.routes).toHaveLength(3);
  expect(update.routes.every((r) => r.kind === "netlink")).toBe(true);
  expect(data.netlinkSockets.entries.size).toBe(3);
  const owned = [...data.netlinkSockets.entries.values()].find(
    (s) =>
      s.endpoint.process_id.pid === a.pid && s.endpoint.resource.inode === "1",
  )!;
  expect(owned.endpoint).toMatchObject({ fd: 3, fd_count: 3 });
  expect(owned.writeBytes).toBe(100);
  expect(owned.readBytes).toBe(0);
  expect(owned.holders).toHaveLength(3);
});

test("file and plate positions share collision-free slots and survive structural updates", () => {
  const fd = endpoint("1"),
    data = store([relation(fd)]);
  data.ingestActivity(
    {
      ...spaceActivity(),
      files: [
        {
          process_id: a,
          file: { device: { major: 8, minor: 1 }, inode: "42", generation: 0 },
          path: "/tmp/example",
          write: true,
          bytes: 1,
          count: 1,
        },
      ],
      ipc: [event(fd)],
    },
    undefined,
    10,
  );
  const filePosition = [...data.filePositions.values()][0],
    platePosition = [...data.netlinkPositions.values()][0];
  expect(platePosition.z).toBeLessThan(0);
  expect(platePosition).not.toEqual(filePosition);
  data.replaceSnapshot(data.snapshot, false, 20);
  expect([...data.filePositions.values()][0]).toEqual(filePosition);
  expect([...data.netlinkPositions.values()][0]).toEqual(platePosition);
});

test("socket closure removes totals immediately, while FD and PID reuse start at zero", () => {
  const old = endpoint("1"),
    data = store([relation(old)]);
  data.ingestActivity({ ...spaceActivity(), ipc: [event(old)] }, undefined, 10);
  const replacement = endpoint("2");
  data.replaceSnapshot(
    { ...data.snapshot, fd_relations: [relation(replacement)] },
    false,
    20,
  );
  expect(
    [...data.netlinkSockets.entries.values()].map((s) => s.writeBytes),
  ).toEqual([0]);
  data.ingestActivity(
    spaceActivity({ ipc: [event(replacement)] }),
    undefined,
    30,
  );
  expect(
    [...data.netlinkSockets.entries.values()].map((s) => s.writeBytes),
  ).toEqual([100]);
  data.replaceSnapshot(
    {
      processes: [processInfo({ identity: { ...a, start_time_ticks: 99 } })],
      fd_relations: [],
    },
    false,
    40,
  );
  expect(data.netlinkSockets.entries.size).toBe(0);
  data.ingestActivity({ ...spaceActivity(), ipc: [event(old)] }, undefined, 50);
  expect(data.netlinkSockets.entries.size).toBe(0);
});

test("invalid observations and filtered owners do not create misleading particle routes", () => {
  const fd = endpoint("1"),
    data = store([relation(fd)]);
  const update = data.ingestActivity(
    {
      ...spaceActivity(),
      ipc: [
        { ...event(fd), bytes: 0 },
        { ...event(fd), count: 0 },
      ],
    },
    undefined,
    10,
  );
  expect(update.routes).toEqual([]);
  expect(data.netlinkSockets.entries.size).toBe(1);
  const filtered = data.ingestActivity(
    { ...spaceActivity(), ipc: [event(fd)] },
    new Set(),
    20,
  );
  expect(filtered.routes).toEqual([]);
  expect(data.netlinkSockets.entries.size).toBe(1);
  data.stop();
  expect(data.netlinkSockets.entries.size).toBe(0);
  expect(data.netlinkPositions.size).toBe(0);
});

test("all current sockets appear beyond the former per-owner and global limits", () => {
  const fds = Array.from({ length: 33 }, (_, i) => endpoint(String(i), i + 3));
  const data = store(fds.map((fd) => relation(fd)));
  expect(data.netlinkSockets.entries.size).toBe(33);
  const order = [...data.netlinkSockets.entries.keys()];
  data.ingestActivity(spaceActivity({ ipc: [event(fds[0])] }));
  expect([...data.netlinkSockets.entries.keys()]).toEqual(order);
  const processes = Array.from({ length: 17 }, (_, i) =>
    processInfo({ identity: { pid: 200 + i, start_time_ticks: 1 } }),
  );
  const many = processes.flatMap((p) =>
    Array.from({ length: 32 }, (_, i) =>
      endpoint(String(p.identity.pid * 32 + i), i + 3, p.identity),
    ),
  );
  data.replaceSnapshot({
    processes,
    fd_relations: many.map((fd) => relation(fd)),
  });
  expect(data.netlinkSockets.entries.size).toBe(544);
  expect(data.netlinkPositions.size).toBe(544);
  expect(
    [...data.netlinkSockets.entries.values()].every((s) => s.writeBytes === 0),
  ).toBe(true);
});

test("the scene suppresses generic netlink relations and renders plates and selectable paths", () => {
  const fd = endpoint("1"),
    usersock = endpoint("2", 4),
    data = store([relation(fd), relation(usersock, 2)]);
  const scene = createSpaceScene();
  const input = () => ({
    nodes: data.nodes,
    snapshot: data.snapshot,
    network: data.network,
    networkPositions: data.networkPositions,
    files: data.recentFiles.entries,
    filePositions: data.filePositions,
    netlink: data.netlinkSockets.entries,
    netlinkPositions: data.netlinkPositions,
    visible: new Set(data.nodes.keys()),
  });
  scene.rebuild(input(), null);
  expect(scene.renderView().edgeViews.map((v) => v.e.id)).toEqual([
    relation(usersock, 2).id,
  ]);
  expect(scene.renderView().netlinkViews.size).toBe(1);
  data.ingestActivity({ ...spaceActivity(), ipc: [event(fd)] }, undefined, 10);
  scene.refreshFiles(input());
  expect(scene.renderView().netlinkViews.size).toBe(1);
  const markers = scene.pickingView().objects.filter((o) => o.userData.netlink);
  expect(markers.map((o) => o.userData.netlink.length)).toEqual([1]);
  expect(
    scene
      .pickingView()
      .objects.some((o) => o.userData.netlinkPaths?.length === 16),
  ).toBe(true);
  scene.refreshFiles({ ...input(), visible: new Set() });
  expect(scene.renderView().netlinkViews.size).toBe(0);
  scene.dispose();
});
