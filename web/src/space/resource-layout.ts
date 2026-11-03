// Group network destinations and calculate stable resource marker coordinates.
// These functions do not own subscriptions, retained activity, or display state.
import { key, remoteLabel } from "./model.js";
import { Display } from "../shared/display.js";
import type { FdRelation, ProcessId } from "../shared/api-types.js";
import type { Position, NetworkGroup, RecentFile } from "./types.js";

export function networkGroups(fd_relations: FdRelation[]) {
  const groups = new Map<string, NetworkGroup>();
  for (const e of fd_relations) {
    if (e.peer || e.shared || !e.socket?.network_peer) continue;
    const id = JSON.stringify([
      key(e.endpoint.process_id),
      Display.protocol(e.socket.protocol),
      e.socket.remote?.ip,
      e.socket.remote?.port,
    ]);
    if (!groups.has(id))
      groups.set(id, {
        id,
        endpoint: e.endpoint,
        socket: e.socket,
        members: [],
        label: "",
      });
    groups.get(id)!.members.push(e);
  }
  for (const group of groups.values()) {
    group.members.sort(
      (a, b) => a.endpoint.fd - b.endpoint.fd || a.id.localeCompare(b.id),
    );
    group.label = `${Display.protocol(group.socket.protocol)} ${remoteLabel(group.socket)}${group.socket.remote_gid ? " · IB" : ""} ×${group.members.length}`;
  }
  return groups;
}

/** Markers anchored to a process, or to an explicit anchor such as an RDMA device. */
type LayoutAnchor =
  { endpoint: { process_id: ProcessId } } | { anchor: string };
const anchorOf = (group: LayoutAnchor) =>
  "anchor" in group ? group.anchor : key(group.endpoint.process_id);

/** Marker cells above each owner; owners on lower host layers carry their `z`. */
export function networkLayout(
  groups: ReadonlyMap<string, LayoutAnchor>,
  positions: ReadonlyMap<string, Pick<Position, "x" | "y"> & { z?: number }>,
  previous: ReadonlyMap<string, Position> = new Map(),
) {
  const result = new Map<string, Position>(),
    occupied = new Set<string>();
  // A global lattice keeps markers apart even for neighboring processes.
  const cell = (p: Position) => `${p.x}:${p.y}:${p.z}`;
  for (const [id] of groups)
    if (previous.has(id)) {
      const p = previous.get(id)!;
      result.set(id, p);
      occupied.add(cell(p));
    }
  for (const [id, group] of [...groups].sort(([a], [b]) =>
    a.localeCompare(b),
  )) {
    if (result.has(id)) continue;
    const origin = positions.get(anchorOf(group));
    if (!origin) continue;
    let placed = false;
    for (let layer = 0; !placed; layer++)
      for (let slot = 0; slot < 9; slot++) {
        const p = {
          x: Math.round(origin.x / 3) * 3 + ((slot % 3) - 1) * 3,
          y: Math.round(origin.y / 3) * 3 + (Math.floor(slot / 3) - 1) * 3,
          z: (origin.z ?? 0) + 18 + layer * 3,
        };
        if (occupied.has(cell(p))) continue;
        result.set(id, p);
        occupied.add(cell(p));
        placed = true;
        break;
      }
  }
  return result;
}

/** File and Netlink cells below each owner, mirrored from the marker lattice. */
export function fileLayout(
  files: ReadonlyMap<string, Pick<RecentFile, "process_id">>,
  positions: ReadonlyMap<string, Pick<Position, "x" | "y"> & { z?: number }>,
  previous: ReadonlyMap<string, Position> = new Map(),
) {
  const groups = new Map(
    [...files].map(([id, f]) => [
      id,
      { endpoint: { process_id: f.process_id } },
    ]),
  );
  // Reflect around 4.5 above the owner's base, which is 0 except on lower host layers.
  const mirror = (id: string, p: Position) => {
    const base = positions.get(key(files.get(id)!.process_id))?.z ?? 0;
    return { ...p, z: 2 * base + 9 - p.z };
  };
  const prior = new Map(
    [...previous]
      .filter(([id]) => files.has(id))
      .map(([id, p]) => [id, mirror(id, p)]),
  );
  return new Map(
    [...networkLayout(groups, positions, prior)].map(([id, p]) => [
      id,
      mirror(id, p),
    ]),
  );
}

if (import.meta.vitest) {
  const { test } = import.meta.vitest;
  test("network grouping and layout", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
    const { fdEndpoint, fdRelation, socketEndpoint } =
      await import("../../tests/support/fixtures.js");
    const { connectionState } = await import("./model.js");
    const netEdge = (
      id: string,
      remote = "203.0.113.1:443",
      pid = 1,
      protocol = "TCP",
    ) => ({
      ...fdRelation(),
      id,
      endpoint: {
        ...fdEndpoint(),
        process_id: { pid, start_time_ticks: 1 },
        resource: {
          kind: "socket" as const,
          device: { major: 0, minor: 0 },
          inode: String(Number(id) || 1),
        },
        fd: Number(id) || 1,
      },
      peer: null,
      shared: false,
      socket: {
        ...socketEndpoint(),
        protocol: {
          kind: protocol.startsWith("UDP")
            ? ("udp" as const)
            : ("tcp" as const),
          family: protocol.endsWith("6")
            ? ("ipv6" as const)
            : ("ipv4" as const),
        },
        state: { kind: "established" as const },
        remote: (() => {
          const i = remote.lastIndexOf(":");
          return {
            ip: remote.slice(0, i).replace(/^\[|\]$/g, ""),
            port: Number(remote.slice(i + 1)),
          };
        })(),
        local: { ip: "127.0.0.1", port: 5000 },
        network_peer: true,
      },
    });
    const connections = [
      netEdge("1"),
      netEdge("2"),
      netEdge("3", "[2001:db8::1]:443"),
      netEdge("4", "203.0.113.1:443", 2),
      netEdge("5", "203.0.113.1:443", 1, "UDP"),
    ];
    const groups = networkGroups(connections);
    assert.equal(groups.size, 4);
    assert.equal([...groups.values()][0].members.length, 2);
    assert.match([...groups.values()][1].label, /\[2001:db8::1\]:443/);
    assert.equal(
      networkGroups([
        { ...connections[0], shared: true },
        { ...connections[0], peer: connections[1].endpoint },
        { ...connections[0], socket: null },
        {
          ...connections[0],
          socket: { ...connections[0].socket, network_peer: false },
        },
      ]).size,
      0,
    );
    const owners = new Map([
      ["1:1", { x: 0, y: 0 }],
      ["2:1", { x: 0, y: 0 }],
    ]);
    const netLayout = networkLayout(groups, owners);
    assert.equal(
      new Set([...netLayout.values()].map((p) => JSON.stringify(p))).size,
      4,
    );
    const grownGroups = networkGroups([
      ...connections,
      netEdge("6", "203.0.113.2:443"),
    ]);
    const netGrown = networkLayout(grownGroups, owners, netLayout);
    for (const [id, p] of netLayout) assert.deepEqual(netGrown.get(id), p);
    assert.deepEqual(networkLayout(new Map(), owners, netGrown), new Map());
    assert.deepEqual(
      networkLayout(networkGroups([...connections].reverse()), owners),
      netLayout,
    );
    assert.equal(
      connectionState({
        ...connections[0],
        socket: {
          ...socketEndpoint(),
          protocol: { kind: "tcp", family: "ipv4" },
          state: { kind: "listen" },
          network_peer: false,
        },
      }),
      "Listening",
    );
    assert.equal(
      connectionState({
        ...connections[0],
        socket: {
          ...socketEndpoint(),
          protocol: { kind: "udp", family: "ipv4" },
          state: { kind: "unconnected" },
          network_peer: false,
        },
      }),
      "No destination set",
    );
    assert.equal(connectionState(connections[0]), "Network destination");
  });
}
