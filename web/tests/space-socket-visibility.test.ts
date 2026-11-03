import { expect, test } from "vitest";

import { TestEventSource } from "./support/mocks.js";
import { SpaceDataStore } from "../src/space/data.js";
import { isSpaceRelationVisible, socketOwnerKey } from "../src/space/model.js";
import type {
  FdRelation,
  IoActivity,
  SocketConnectionStatus,
} from "../src/shared/api-types.js";
import {
  fdEndpoint,
  fdRelation,
  processInfo,
  socketEndpoint,
  spaceActivity,
} from "./support/fixtures.js";

function socketRelation(
  status?: SocketConnectionStatus,
  id = "socket",
): FdRelation {
  return fdRelation({
    id,
    endpoint: fdEndpoint({
      kind: "socket",
      resource: { kind: "socket", device: { major: 0, minor: 10 }, inode: id },
      connection_status: status,
    }),
    socket: socketEndpoint(),
  });
}
function activity(relation: FdRelation): IoActivity {
  return {
    process_id: relation.endpoint.process_id,
    resource: relation.endpoint.resource,
    write: true,
    bytes: 128,
    count: 1,
  };
}

test.each(["listening", "unconnected"] as const)(
  "keeps %s sockets in observations and excludes their relations and traffic",
  (status) => {
    const store = new SpaceDataStore();
    const external = socketRelation(status);
    const shared = {
      ...external,
      id: "shared",
      shared: true,
      peer: {
        ...external.endpoint,
        process_id: { pid: 2, start_time_ticks: 1 },
      },
    };
    const pipe = fdRelation();
    store.replaceSnapshot({
      processes: [
        processInfo(),
        processInfo({ identity: shared.peer.process_id }),
      ],
      fd_relations: [external, shared, pipe],
    });
    expect(store.snapshot.fd_relations).toHaveLength(3);
    expect(store.displaySnapshot.fd_relations).toEqual([pipe]);
    const result = store.ingestActivity(
      spaceActivity({
        ipc: [
          activity(external),
          activity({ ...shared, endpoint: shared.peer }),
        ],
      }),
    );
    expect(result.routes).toEqual([]);
    expect(store.edgeStats.size).toBe(0);
    // Kind/connection metadata may be independent: socket:null must still be excluded.
    expect(isSpaceRelationVisible({ ...external, socket: null })).toBe(false);
  },
);

test.each(["peer_present", "unknown", undefined] as const)(
  "preserves sockets with status %s including old server payloads",
  (status) => {
    const store = new SpaceDataStore();
    const relation = socketRelation(status);
    store.replaceSnapshot({
      processes: [processInfo()],
      fd_relations: [relation],
    });
    expect(store.displaySnapshot.fd_relations).toEqual([relation]);
    expect(
      store.ingestActivity(spaceActivity({ ipc: [activity(relation)] }))
        .routes[0].kind,
    ).toBe("connection");
  },
);

test("excludes either socket endpoint without suppressing unrelated pipe or socket traffic", () => {
  const store = new SpaceDataStore();
  const connected = socketRelation("peer_present");
  const excluded = socketRelation("unconnected", "receiver");
  const relation = { ...connected, peer: excluded.endpoint };
  const visible = socketRelation("unknown", "visible");
  const pipe = fdRelation();
  expect(
    isSpaceRelationVisible({
      ...pipe,
      endpoint: { ...pipe.endpoint, connection_status: "listening" },
    }),
  ).toBe(true);
  store.replaceSnapshot({
    processes: [processInfo()],
    fd_relations: [relation, visible, pipe],
  });
  expect(store.displaySnapshot.fd_relations).toEqual([visible, pipe]);
  expect(
    store.ingestActivity(
      spaceActivity({ ipc: [activity(connected), activity(excluded)] }),
    ).routes,
  ).toEqual([]);
  expect(
    store.ingestActivity(
      spaceActivity({ ipc: [activity(visible), activity(pipe)] }),
    ).routes,
  ).toHaveLength(2);
});

test("a status-only delta clears activity and restores a socket when its peer becomes available", () => {
  const source = new TestEventSource("/api/system/events");
  const store = new SpaceDataStore(undefined, undefined, () =>
    source.asEventSource(),
  );
  store.start();
  const relation = socketRelation("peer_present");
  source.emit("snapshot", {
    kind: "full",
    processes: [processInfo()],
    fd_relations: [relation],
    sequence: 1,
  });
  store.ingestActivity(spaceActivity({ ipc: [activity(relation)] }));
  expect(store.edgeStats.size).toBe(1);
  const excluded = {
    ...relation,
    endpoint: {
      ...relation.endpoint,
      connection_status: "unconnected" as const,
    },
  };
  source.emit("snapshot", {
    kind: "delta",
    sequence: 2,
    base_sequence: 1,
    processes: [],
    fd_relations_delta: { upsert: [excluded], remove: [] },
  });
  expect(store.snapshot.fd_relations).toEqual([excluded]);
  expect(store.displaySnapshot.fd_relations).toEqual([]);
  expect(store.edgeStats.size).toBe(0);
  expect(
    store.suppressedSocketOwners.has(socketOwnerKey(relation.endpoint)),
  ).toBe(true);
  store.replaceSnapshot({ ...store.snapshot, fd_relations: [relation] });
  expect(store.suppressedSocketOwners.size).toBe(0);
  expect(store.displaySnapshot.fd_relations).toEqual([relation]);
  store.stop();
});

test("Netlink sockets are removed immediately on closure or explicit exclusion", () => {
  const store = new SpaceDataStore();
  const relation = {
    ...socketRelation("peer_present"),
    socket: socketEndpoint({
      protocol: {
        kind: "other" as const,
        family: 16,
        socket_type: { kind: "dgram" as const },
        protocol: 0,
      },
    }),
  };
  const snapshot = { processes: [processInfo()], fd_relations: [relation] };
  store.replaceSnapshot(snapshot, false, 100);
  store.ingestActivity(
    spaceActivity({ ipc: [activity(relation)] }),
    undefined,
    100,
  );
  expect(store.netlinkSockets.entries.size).toBe(1);
  store.replaceSnapshot({ ...snapshot, fd_relations: [] }, false, 200);
  expect(store.netlinkSockets.entries.size).toBe(0);
  expect(store.netlinkPositions.size).toBe(0);
  store.replaceSnapshot(snapshot, false, 250);
  expect(store.netlinkSockets.entries.size).toBe(1);
  const excluded = {
    ...relation,
    endpoint: {
      ...relation.endpoint,
      connection_status: "unconnected" as const,
    },
  };
  store.replaceSnapshot({ ...snapshot, fd_relations: [excluded] }, false, 300);
  expect(store.netlinkSockets.entries.size).toBe(0);
  expect(store.netlinkPositions.size).toBe(0);
  expect(
    store.ingestActivity(spaceActivity({ ipc: [activity(excluded)] })).routes,
  ).toEqual([]);
});
