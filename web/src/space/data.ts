// System SSE, observed state, retained activity, and plain layout coordinates.
import { spaceNow } from "./clock.js";
import { NetlinkSocketStore, isKernelNetlink } from "./netlink.js";
import { Display } from "../shared/display.js";
import {
  key,
  remoteLabel,
  ipcKey,
  fileLabel,
  layoutMaps,
  excludedSocket,
  isSpaceRelationVisible,
  socketOwnerKey,
} from "./model.js";
export { layoutMaps };
import { treeLayout, stableLayout } from "./process-layout.js";
import { emptyActivityRetention } from "./types.js";
import type {
  Position,
  NetworkGroup,
  RecentFile,
  CpuGlow,
  PlacedProcess,
  EdgeStat,
  ActivityRoute,
  ActivityUpdate,
  ActivityRetention,
  DataEvents,
} from "./types.js";
import type {
  ProcessId,
  SpaceActivity,
  FdRelation,
  IoActivity,
  FileActivity,
  IpcIdentity,
  MemoryMap,
  SystemSnapshot,
  SystemSnapshotUpdate,
} from "../shared/api-types.js";

function networkGroups(fd_relations: FdRelation[]) {
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
    group.label = `${Display.protocol(group.socket.protocol)} ${remoteLabel(group.socket)} ×${group.members.length}`;
  }
  return groups;
}

function networkLayout(
  groups: ReadonlyMap<string, { endpoint: { process_id: ProcessId } }>,
  positions: ReadonlyMap<string, Pick<Position, "x" | "y">>,
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
    const origin = positions.get(key(group.endpoint.process_id));
    if (!origin) continue;
    let placed = false;
    for (let layer = 0; !placed; layer++)
      for (let slot = 0; slot < 9; slot++) {
        const p = {
          x: Math.round(origin.x / 3) * 3 + ((slot % 3) - 1) * 3,
          y: Math.round(origin.y / 3) * 3 + (Math.floor(slot / 3) - 1) * 3,
          z: 12 + layer * 3,
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

function edgeDirection(
  edge: Pick<FdRelation, "endpoint" | "peer" | "shared">,
  event: Pick<IoActivity, "process_id" | "resource" | "write">,
) {
  const a =
    key(edge.endpoint.process_id) === key(event.process_id) &&
    ipcKey(edge.endpoint.resource) === ipcKey(event.resource);
  const b =
    edge.peer &&
    key(edge.peer.process_id) === key(event.process_id) &&
    ipcKey(edge.peer.resource) === ipcKey(event.resource);
  if (edge.shared || (!a && !b) || (a && b)) return null;
  return a ? (event.write ? 1 : -1) : event.write ? -1 : 1;
}

// Recent file activity is retained across structural snapshot updates.
const fileKey = (event: Pick<FileActivity, "process_id" | "file">) =>
  JSON.stringify([
    key(event.process_id),
    event.file.device.major,
    event.file.device.minor,
    event.file.inode,
    event.file.generation,
  ]);
/** @inline */
class RecentFiles {
  entries = new Map<string, RecentFile>();
  evicted = 0;
  clear() {
    this.entries.clear();
    this.evicted = 0;
  }
  prune(
    now: number,
    live: ReadonlySet<string>,
    retained: ReadonlySet<string> = new Set(),
  ) {
    let changed = false;
    for (const [id, file] of this.entries)
      if (
        !retained.has(id) &&
        (now - file.last >= 30000 || !live.has(key(file.process_id)))
      ) {
        this.entries.delete(id);
        changed = true;
      }
    return changed;
  }
  ingest(
    events: FileActivity[],
    now: number,
    live: ReadonlySet<string>,
    retained: ReadonlySet<string> = new Set(),
  ) {
    let changed = this.prune(now, live, retained);
    for (const e of events) {
      if (!live.has(key(e.process_id)) || !(e.bytes > 0) || !(e.count > 0))
        continue;
      const id = fileKey(e);
      let file = this.entries.get(id);
      if (!file) {
        const owner = key(e.process_id),
          owned = [...this.entries.values()].filter(
            (f) => key(f.process_id) === owner,
          );
        const victim =
          owned.length >= 32
            ? owned[0]
            : this.entries.size >= 512
              ? this.entries.values().next().value
              : null;
        if (victim) {
          this.entries.delete(victim.id);
          this.evicted++;
        }
        file = {
          id,
          process_id: e.process_id,
          file: e.file,
          path: null,
          label: fileLabel(e.file),
          last: now,
          readBytes: 0,
          writeBytes: 0,
          readCount: 0,
          writeCount: 0,
        };
        changed = true;
      }
      if (e.path) file.path = e.path;
      file.label = file.path?.split("/").pop() || fileLabel(file.file);
      file.last = now;
      file[e.write ? "writeBytes" : "readBytes"] += e.bytes;
      file[e.write ? "writeCount" : "readCount"] += e.count;
      this.entries.delete(id);
      this.entries.set(id, file);
    }
    return changed;
  }
}
function fileLayout(
  files: ReadonlyMap<string, Pick<RecentFile, "process_id">>,
  positions: ReadonlyMap<string, Pick<Position, "x" | "y">>,
  previous: ReadonlyMap<string, Position> = new Map(),
) {
  const groups = new Map(
    [...files].map(([id, f]) => [
      id,
      { endpoint: { process_id: f.process_id } },
    ]),
  );
  const prior = new Map(
    [...previous].map(([id, p]) => [id, { ...p, z: 9 - p.z }]),
  );
  return new Map(
    [...networkLayout(groups, positions, prior)].map(([id, p]) => [
      id,
      { ...p, z: 9 - p.z },
    ]),
  );
}

/** Omitted maps retain the previous mapping for this exact process identity. */
function mergeSnapshot(
  previous: SystemSnapshot,
  update: SystemSnapshotUpdate,
): SystemSnapshot {
  if (update.kind !== "full" && update.kind !== "delta")
    throw new Error("Unknown snapshot kind");
  if (
    update.kind === "delta" &&
    (previous.sequence === undefined ||
      update.base_sequence !== previous.sequence)
  )
    throw new Error("Snapshot baseline mismatch");
  const known = new Map(previous.processes.map((p) => [key(p.identity), p]));
  const relations = new Map(
    (update.kind === "full" ? [] : previous.fd_relations).map((r) => [r.id, r]),
  );
  if (update.fd_relations_delta) {
    for (const id of update.fd_relations_delta.remove) relations.delete(id);
    for (const r of update.fd_relations_delta.upsert) relations.set(r.id, r);
  }
  return {
    ...update,
    fd_relations: update.fd_relations ?? [...relations.values()],
    processes: update.processes.map((p) => {
      const old =
        update.kind === "full" ? undefined : known.get(key(p.identity));
      let maps = p.maps ?? old?.maps ?? [];
      if (p.maps_delta) {
        if (!old) throw new Error("Missing process maps baseline");
        const entries = new Map(old.maps.map((m) => [m.start, m]));
        for (const address of p.maps_delta.remove) entries.delete(address);
        for (const m of p.maps_delta.upsert) entries.set(m.start, m);
        maps = [...entries.values()].sort((a, b) => {
          const startA = BigInt(a.start),
            startB = BigInt(b.start);
          return startA < startB ? -1 : startA > startB ? 1 : 0;
        });
      }
      const { maps_delta: _delta, ...process } = p;
      return {
        ...process,
        maps,
        maps_epoch:
          p.maps === undefined && p.maps_delta === undefined
            ? (old?.maps_epoch ?? 0)
            : p.maps_epoch,
      };
    }),
  };
}

const quietEvents: DataEvents = {
  snapshot() {},
  activity() {},
  reset() {},
  gap() {},
  status() {},
};

/** Owns the system subscription, observations, retained activity, and layout. */
export class SpaceDataStore {
  snapshot: SystemSnapshot = { processes: [], fd_relations: [] };
  /** Raw observations remain in snapshot; this view is used throughout SPACE. */
  displaySnapshot: SystemSnapshot = this.snapshot;
  readonly suppressedSocketOwners = new Set<string>();
  readonly nodes = new Map<string, PlacedProcess>();
  readonly cpuGlows = new Map<string, CpuGlow>();
  readonly edgeStats = new Map<string, EdgeStat>();
  readonly recentFiles = new RecentFiles();
  readonly netlinkSockets = new NetlinkSocketStore();
  network = new Map<string, NetworkGroup>();
  networkPositions = new Map<string, Position>();
  filePositions = new Map<string, Position>();
  netlinkPositions = new Map<string, Position>();
  private source: EventSource | null = null;
  private retry: ReturnType<typeof setTimeout> | undefined;
  private running = false;
  private liveIds = new Set<string>();
  private liveRelations = new Set<string>();
  private retained = emptyActivityRetention();

  constructor(
    private readonly events: DataEvents = quietEvents,
    private readonly visibleIds: () => ReadonlySet<string> = () =>
      new Set(this.nodes.keys()),
    private readonly openEvents: (url: string) => EventSource = (url) =>
      new EventSource(url),
    private readonly activeResources: (
      now: number,
      suppressed: ReadonlySet<string>,
    ) => ActivityRetention = emptyActivityRetention,
    private readonly observe: (
      event: import("./recording.js").RecordingEvent,
    ) => void = () => {},
  ) {}

  replaceSnapshot(
    snapshot: SystemSnapshot,
    rearrange = false,
    now = spaceNow(),
  ) {
    this.snapshot = snapshot;
    this.liveIds = new Set(snapshot.processes.map((p) => key(p.identity)));
    this.liveRelations = new Set(snapshot.fd_relations.map((r) => r.id));
    this.suppressedSocketOwners.clear();
    const visibleOwners = new Set<string>();
    const excludedOwners = new Set<string>();
    for (const relation of snapshot.fd_relations) {
      const shown = isSpaceRelationVisible(relation);
      for (const endpoint of [relation.endpoint, relation.peer]) {
        if (!endpoint || endpoint.resource.kind !== "socket") continue;
        const owner = socketOwnerKey(endpoint);
        if (excludedSocket(endpoint)) this.suppressedSocketOwners.add(owner);
        (shown ? visibleOwners : excludedOwners).add(owner);
      }
    }
    // A relation removed because of its other endpoint must not create a port fallback.
    for (const owner of excludedOwners)
      if (!visibleOwners.has(owner)) this.suppressedSocketOwners.add(owner);
    this.updateDisplay(now, rearrange);
  }

  private updateDisplay(now: number, rearrange = false) {
    const retained = this.activeResources(now, this.suppressedSocketOwners);
    const processes = [
      ...this.snapshot.processes,
      ...this.displaySnapshot.processes.filter(
        (p) =>
          !this.liveIds.has(key(p.identity)) &&
          retained.processes.has(key(p.identity)),
      ),
    ];
    const currentRelations = this.snapshot.fd_relations.filter(
      isSpaceRelationVisible,
    );
    const relations = [
      ...currentRelations,
      ...this.displaySnapshot.fd_relations.filter(
        (r) =>
          !this.liveRelations.has(r.id) &&
          retained.connections.has(r.id) &&
          [r.endpoint, r.peer].every(
            (e) => !e || !this.suppressedSocketOwners.has(socketOwnerKey(e)),
          ),
      ),
    ];
    this.displaySnapshot = {
      ...this.snapshot,
      processes,
      fd_relations: relations,
    };
    const live = new Set(processes.map((p) => key(p.identity)));
    for (const id of this.nodes.keys())
      if (!live.has(id)) this.cpuGlows.delete(id);
    const edges = new Set(relations.map((e) => e.id));
    for (const id of this.edgeStats.keys())
      if (!edges.has(id)) this.edgeStats.delete(id);
    const layout = rearrange
      ? treeLayout(processes, undefined, undefined, relations)
      : stableLayout(
          processes,
          this.positions(),
          undefined,
          undefined,
          relations,
        );
    this.nodes.clear();
    for (const p of processes) {
      const place = layout.get(key(p.identity));
      this.nodes.set(key(p.identity), {
        ...p,
        pos: { x: place?.x ?? 0, y: place?.y ?? 0, z: 0 },
        regions: layoutMaps(p.maps),
      });
    }
    this.recentFiles.prune(now, this.liveIds, retained.files);
    this.netlinkSockets.sync(currentRelations, this.liveIds, retained.netlink);
    this.network = networkGroups(relations);
    this.networkPositions = networkLayout(
      this.network,
      this.positions(),
      rearrange ? new Map() : this.networkPositions,
    );
    if (rearrange) {
      this.filePositions.clear();
      this.netlinkPositions.clear();
    }
    this.layoutFiles();
    this.retained = {
      processes: new Set(
        processes
          .map((p) => key(p.identity))
          .filter((id) => !this.liveIds.has(id)),
      ),
      connections: new Set(
        relations.map((r) => r.id).filter((id) => !this.liveRelations.has(id)),
      ),
      networks: new Set(),
      files: new Set(
        [...this.recentFiles.entries]
          .filter(
            ([, f]) =>
              !this.liveIds.has(key(f.process_id)) || now - f.last >= 30000,
          )
          .map(([id]) => id),
      ),
      netlink: new Set(
        [...this.netlinkSockets.entries.keys()].filter(
          (id) => !this.netlinkSockets.currentIds.has(id),
        ),
      ),
    };
  }

  /** Rebuild only when an animation no longer needs an absent observation. */
  pruneRetention(now = spaceNow()) {
    if (!Object.values(this.retained).some((ids) => ids.size)) return false;
    const active = this.activeResources(now, this.suppressedSocketOwners);
    for (const kind of [
      "processes",
      "connections",
      "files",
      "netlink",
    ] as const)
      for (const id of this.retained[kind])
        if (!active[kind].has(id)) {
          this.updateDisplay(now);
          return true;
        }
    return false;
  }
  private positions() {
    return new Map([...this.nodes].map(([id, n]) => [id, n.pos]));
  }
  private layoutFiles() {
    const resources = new Map<string, Pick<RecentFile, "process_id">>(
      this.recentFiles.entries,
    );
    for (const [id, socket] of this.netlinkSockets.entries)
      resources.set(id, { process_id: socket.endpoint.process_id });
    const positions = fileLayout(
      resources,
      this.positions(),
      new Map([...this.filePositions, ...this.netlinkPositions]),
    );
    this.filePositions = new Map(
      [...positions].filter(([id]) => this.recentFiles.entries.has(id)),
    );
    this.netlinkPositions = new Map(
      [...positions].filter(([id]) => this.netlinkSockets.entries.has(id)),
    );
  }

  /** Matches the relations included in the scene for the current search. */
  private visibleRelations(visible: ReadonlySet<string>) {
    const grouped = new Set(
      [...this.network.values()].flatMap((g) => g.members.map((e) => e.id)),
    );
    return this.snapshot.fd_relations.filter((e) => {
      if (!isSpaceRelationVisible(e)) return false;
      if (
        !visible.has(key(e.endpoint.process_id)) ||
        !this.nodes.has(key(e.endpoint.process_id))
      )
        return false;
      if (grouped.has(e.id)) return true;
      const peer = e.peer && key(e.peer.process_id);
      return !peer || !this.nodes.has(peer) || visible.has(peer);
    });
  }
  ingestActivity(
    activity: SpaceActivity,
    visible: ReadonlySet<string> = this.visibleIds(),
    now = spaceNow(),
  ): ActivityUpdate {
    const live = this.liveIds;
    const filesChanged = this.recentFiles.ingest(
      activity.files || [],
      now,
      live,
      this.activeResources(now, this.suppressedSocketOwners).files,
    );
    const routes: ActivityRoute[] = [];
    for (const e of activity.files || []) {
      const id = fileKey(e);
      if (
        live.has(key(e.process_id)) &&
        visible.has(key(e.process_id)) &&
        this.recentFiles.entries.has(id) &&
        e.bytes > 0 &&
        e.count > 0
      )
        routes.push({
          kind: "file",
          id,
          direction: e.write ? 1 : -1,
          count: e.count,
        });
    }
    for (const e of activity.cpu || []) {
      const id = key(e.process_id);
      if (live.has(id))
        this.cpuGlows.set(id, {
          ...e,
          window_ms: activity.window_ms || 100,
          last: now,
        });
    }
    const relations = this.visibleRelations(visible);
    for (const e of activity.ipc || []) {
      if (!live.has(key(e.process_id))) continue;
      const owner =
        e.resource.kind === "socket" ? socketOwnerKey(e) : undefined;
      if (owner && this.suppressedSocketOwners.has(owner)) continue;
      const netlink = this.netlinkSockets.ingest(e, live);
      if (netlink) {
        if (netlink.observed && visible.has(key(e.process_id)))
          routes.push({
            kind: "netlink",
            id: netlink.id,
            direction: e.write ? 1 : -1,
            count: e.count,
            socketOwnerKey: owner,
          });
        continue;
      }
      const links = relations.filter(
        (r) => !isKernelNetlink(r.socket) && edgeDirection(r, e) !== null,
      );
      const exact = links.filter((r) => !r.candidate);
      const candidates = exact.length ? exact : links;
      if (candidates.length !== 1) {
        if (this.nodes.has(key(e.process_id)))
          routes.push({
            kind: "port",
            id: key(e.process_id),
            count: e.count,
            socketOwnerKey: owner,
          });
        continue;
      }
      const relation = candidates[0],
        prior = this.edgeStats.get(relation.id);
      this.edgeStats.set(relation.id, {
        bytes: e.bytes + (prior?.time === now ? prior.bytes : 0),
        count: e.count + (prior?.time === now ? prior.count : 0),
        time: now,
      });
      routes.push({
        kind: "connection",
        socketOwnerKey: owner,
        id: relation.id,
        direction: edgeDirection(relation, e),
        count: e.count,
      });
    }
    const signals = (activity.signals || [])
      .filter((event) => {
        const source = key(event.source_id),
          destination = key(event.destination_id);
        return (
          live.has(source) &&
          live.has(destination) &&
          visible.has(source) &&
          visible.has(destination)
        );
      })
      .slice(-256);
    if (filesChanged) this.layoutFiles();
    return { now, filesChanged, netlinkChanged: false, routes, signals };
  }
  pruneFiles(now = spaceNow()) {
    const changed = this.recentFiles.prune(
      now,
      this.liveIds,
      this.activeResources(now, this.suppressedSocketOwners).files,
    );
    if (changed) this.layoutFiles();
    return changed;
  }
  start() {
    this.running = true;
    this.connect();
  }
  private connect() {
    if (!this.running || this.source) return;
    clearTimeout(this.retry);
    const current = this.openEvents("/api/system/events");
    this.source = current;
    current.onopen = () => {
      if (this.source === current) this.events.status(null);
    };
    current.onerror = () => {
      if (this.source !== current) return;
      this.close();
      this.events.status("Connection lost. Retrying…");
      if (this.running) this.retry = setTimeout(() => this.connect(), 3000);
    };
    current.addEventListener("snapshot", (event) => {
      if (this.source !== current) return;
      let merged: SystemSnapshot;
      try {
        merged = mergeSnapshot(this.snapshot, JSON.parse(event.data));
      } catch {
        this.close();
        this.connect();
        return;
      }
      this.replaceSnapshot(merged);
      this.observe({ type: "snapshot", at_ms: 0, data: merged });
      this.events.snapshot();
    });
    current.addEventListener("activity", (event) => {
      if (this.source === current) {
        const activity = JSON.parse(event.data);
        this.observe({ type: "activity", at_ms: 0, data: activity });
        this.events.activity(this.ingestActivity(activity));
      }
    });
    current.addEventListener("gap", () => {
      if (this.source === current) this.events.gap();
    });
  }
  private close() {
    clearTimeout(this.retry);
    this.source?.close();
    this.source = null;
    this.cpuGlows.clear();
    this.recentFiles.clear();
    this.netlinkSockets.clear();
    this.layoutFiles();
    this.events.reset();
  }
  stop() {
    this.running = false;
    this.close();
  }
}

if (import.meta.vitest) {
  const { test } = import.meta.vitest;
  test("memory layout and edge direction", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
    const { processInfo, memoryMap, fdEndpoint, fdRelation, socketEndpoint } =
      await import("../../tests/support/fixtures.js");
    const regions = layoutMaps([
      memoryMap({ start: "0xffffffffff600000", end: "0xffffffffff601000" }),
      memoryMap({ start: "0x1000", end: "0x3000" }),
      memoryMap({ start: "0x7fff00000000", end: "0x7fff00001000" }),
    ]);
    assert.equal(regions[0].start, "0x1000");
    assert.ok(regions[2].z > regions[1].z);
    assert.ok(regions.every((region) => region.h > 0));
    assert.deepEqual(layoutMaps([]), []);
    const a = fdEndpoint({
        process_id: { pid: 1, start_time_ticks: 2 },
        resource: { kind: "pipe", device: { major: 0, minor: 1 }, inode: "2" },
      }),
      b = fdEndpoint({
        process_id: { pid: 2, start_time_ticks: 3 },
        resource: { kind: "pipe", device: { major: 0, minor: 1 }, inode: "2" },
      });
    const edge = { endpoint: a, peer: b, shared: false };
    assert.equal(edgeDirection(edge, { ...a, write: true }), 1);
    assert.equal(edgeDirection(edge, { ...b, write: false }), 1);
    assert.equal(
      edgeDirection({ ...edge, shared: true }, { ...a, write: true }),
      null,
    );
    assert.equal(
      edgeDirection(edge, {
        ...a,
        process_id: { pid: 1, start_time_ticks: 999 },
        write: true,
      }),
      null,
    );
    {
      const resource: IpcIdentity = {
        kind: "pipe",
        device: { major: 8, minor: 1 },
        inode: "18446744073709551615",
      };
      const endpoint = fdEndpoint({
        process_id: { pid: 1, start_time_ticks: 2 },
        resource,
      });
      assert.equal(
        edgeDirection(
          { endpoint, peer: null, shared: false },
          {
            ...endpoint,
            resource: {
              inode: resource.inode,
              device: { minor: 1, major: 8 },
              kind: "pipe",
            },
            write: true,
          },
        ),
        1,
      );
      assert.equal(
        edgeDirection(
          { endpoint, peer: null, shared: false },
          {
            ...endpoint,
            resource: { ...resource, inode: "18446744073709551614" },
            write: true,
          },
        ),
        null,
      );
      const base = {
        process_id: endpoint.process_id,
        file: { device: resource.device, inode: resource.inode, generation: 0 },
      };
      for (const file of [
        { ...base.file, inode: "18446744073709551614" },
        { ...base.file, generation: 1 },
        { ...base.file, device: { major: 9, minor: 1 } },
        { ...base.file, device: { major: 8, minor: 2 } },
      ])
        assert.notEqual(fileKey({ ...base, file }), fileKey(base));
    }
  });
  test("network grouping and layout", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
    const { processInfo, memoryMap, fdEndpoint, fdRelation, socketEndpoint } =
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
    assert.equal(
      edgeDirection(connections[0], {
        ...connections[0].endpoint,
        write: true,
      }),
      1,
    );
    assert.equal(
      edgeDirection(connections[0], {
        ...connections[0].endpoint,
        write: false,
      }),
      -1,
    );
  });
  test("recent files and placement", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
    {
      const files = new RecentFiles(),
        owner = { pid: 1, start_time_ticks: 1 },
        live = new Set(["1:1"]);
      const event = {
        process_id: owner,
        file: { device: { major: 8, minor: 1 }, inode: "42", generation: 0 },
        path: "/tmp/example",
        write: false,
        bytes: 7,
        count: 1,
      };
      assert.equal(
        files.ingest([event, { ...event, write: true, bytes: 11 }], 100, live),
        true,
      );
      const id = fileKey(event),
        file = files.entries.get(id)!;
      assert.equal(file.readBytes, 7);
      assert.equal(file.writeBytes, 11);
      assert.equal(file.label, "example");
      const positions = new Map([["1:1", { x: 0, y: 0 }]]),
        before = fileLayout(files.entries, positions);
      files.ingest([{ ...event, path: "/tmp/renamed" }], 200, live);
      assert.deepEqual(
        fileLayout(files.entries, positions, before),
        before,
        "rename preserves placement",
      );
      assert.equal(file.label, "renamed");
      files.ingest(
        [
          { ...event, process_id: { pid: 1, start_time_ticks: 2 } },
          { ...event, bytes: 0 },
        ],
        300,
        live,
      );
      assert.equal(
        files.entries.size,
        1,
        "stale process and empty events ignored",
      );
      for (let i = 0; i < 35; i++)
        files.ingest(
          [
            {
              ...event,
              file: {
                device: { major: 8, minor: 1 },
                inode: String(i + 1000),
                generation: 0,
              },
              path: null,
            },
          ],
          400 + i,
          live,
        );
      assert.equal(files.entries.size, 32);
      assert.equal(files.evicted, 4);
      const stable = fileLayout(files.entries, positions, before);
      assert.equal(
        new Set([...stable.values()].map((p) => JSON.stringify(p))).size,
        32,
        "markers never overlap",
      );
      assert.ok(
        [...stable.values()].every((p) => p.z < 0),
        "files occupy separate space below the process",
      );
      assert.equal(files.prune(30433, live), true);
      assert.equal(files.entries.size, 1);
      assert.equal(files.prune(30434, live), true);
      assert.equal(files.entries.size, 0);
      for (let p = 1; p <= 20; p++) {
        live.add(`${p}:1`);
        for (let i = 0; i < 32; i++)
          files.ingest(
            [
              {
                ...event,
                process_id: { pid: p, start_time_ticks: 1 },
                file: {
                  device: { major: 8, minor: 1 },
                  inode: String(i + 1000),
                  generation: 0,
                },
              },
            ],
            40000,
            live,
          );
      }
      assert.equal(files.entries.size, 512, "global display limit enforced");
      files.prune(40001, new Set(["20:1"]));
      assert.equal(
        files.entries.size,
        32,
        "removed processes lose file markers",
      );
      files.clear();
      assert.equal(files.entries.size, 0);
      assert.equal(files.evicted, 0);
    }
  });
  test("snapshot merging", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
    const { processInfo, memoryMap, fdEndpoint, fdRelation, socketEndpoint } =
      await import("../../tests/support/fixtures.js");
    const original = processInfo({
      identity: { pid: 1, start_time_ticks: 10 },
      maps: [memoryMap({ start: "0x1000", end: "0x2000" })],
      maps_epoch: 10,
      maps_error: null,
    });
    const base = { processes: [original], fd_relations: [], sequence: 1 };
    const delta = (
      extra: Pick<SystemSnapshotUpdate, "processes">,
    ): SystemSnapshotUpdate => ({
      kind: "delta",
      sequence: 2,
      base_sequence: 1,
      fd_relations: [],
      ...extra,
    });
    const { maps: _maps, ...originalWithoutMaps } = original;
    const omitted = {
      ...originalWithoutMaps,
      identity: original.identity,
      maps_epoch: 20,
      maps_error: "read failed",
    };
    const retained = mergeSnapshot(base, delta({ processes: [omitted] }));
    assert.deepEqual(retained.processes[0].maps, original.maps);
    assert.equal(retained.processes[0].maps_epoch, 10);
    assert.equal(retained.processes[0].maps_error, "read failed");
    assert.deepEqual(
      mergeSnapshot(base, delta({ processes: [{ ...omitted, maps: [] }] }))
        .processes[0].maps,
      [],
    );
    assert.deepEqual(
      mergeSnapshot(base, delta({ processes: [] })).processes,
      [],
    );
    assert.deepEqual(
      mergeSnapshot(
        base,
        delta({
          processes: [
            { ...omitted, identity: { pid: 1, start_time_ticks: 11 } },
          ],
        }),
      ).processes[0].maps,
      [],
    );
    assert.equal(
      mergeSnapshot(retained, { ...base, kind: "full" }).processes[0]
        .maps_epoch,
      10,
    );

    const changedMap = memoryMap({
      start: "0x1000",
      end: "0x1800",
      writable: true,
    });
    const added = memoryMap({ start: "0x1800", end: "0x2000" });
    const split = mergeSnapshot(
      base,
      delta({
        processes: [
          {
            ...omitted,
            maps_delta: { upsert: [added, changedMap], remove: [] },
          },
        ],
      }),
    );
    assert.deepEqual(split.processes[0].maps, [changedMap, added]);
    assert.equal(split.processes[0].maps_epoch, 20);
    const joined = mergeSnapshot(split, {
      kind: "delta",
      sequence: 3,
      base_sequence: 2,
      processes: [
        {
          ...omitted,
          maps_delta: { upsert: [original.maps[0]], remove: ["0x1800"] },
        },
      ],
      fd_relations_delta: {
        upsert: [fdRelation({ id: "new", label: "pipe" })],
        remove: [],
      },
    });
    assert.deepEqual(joined.processes[0].maps, original.maps);
    assert.deepEqual(joined.fd_relations, [
      fdRelation({ id: "new", label: "pipe" }),
    ]);
    const removed = mergeSnapshot(joined, {
      kind: "delta",
      sequence: 4,
      base_sequence: 3,
      processes: [],
      fd_relations_delta: { upsert: [], remove: ["new"] },
    });
    assert.deepEqual(removed.fd_relations, []);
    assert.throws(
      () =>
        mergeSnapshot(base, {
          kind: "delta",
          sequence: 9,
          base_sequence: 8,
          processes: [],
        }),
      /baseline mismatch/,
    );
    assert.throws(
      () =>
        mergeSnapshot(
          base,
          delta({
            processes: [
              {
                ...omitted,
                identity: { pid: 1, start_time_ticks: 99 },
                maps_delta: { upsert: [], remove: [] },
              },
            ],
          }),
        ),
      /Missing process maps baseline/,
    );
    assert.deepEqual(
      mergeSnapshot(joined, {
        kind: "full",
        sequence: 10,
        processes: [],
        fd_relations: [],
      }).fd_relations,
      [],
    );
  });
  test("space-data regression", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
    const { processInfo, fdEndpoint, fdRelation, spaceActivity, cpuActivity } =
      await import("../../tests/support/fixtures.js");
    type TestEventSource =
      import("../../tests/support/mocks.js").TestEventSource;
    const { TestEventSource } = await import("../../tests/support/mocks.js");
    const a = { pid: 101, start_time_ticks: 1 },
      b = { pid: 102, start_time_ticks: 2 };
    const process = (
      identity: ProcessId,
      name: string,
      parent_id: ProcessId | null = null,
    ) =>
      processInfo({
        identity,
        name,
        parent_id,
        maps: [],
      });
    const resource: IpcIdentity = {
      kind: "pipe",
      device: { major: 0, minor: 1 },
      inode: "7",
    };
    const endpoint = (process_id: ProcessId) =>
      fdEndpoint({ process_id, fd: 3, resource });
    const relation = fdRelation({
      id: "pipe",
      endpoint: endpoint(a),
      peer: endpoint(b),
      candidate: false,
      shared: false,
    });
    const initial = {
      processes: [process(a, "writer"), process(b, "reader", a)],
      fd_relations: [relation],
    };
    const store = new SpaceDataStore();
    store.replaceSnapshot(initial, false, 1000);
    assert.deepEqual(Object.keys(store.nodes.get(key(a))!.pos).sort(), [
      "x",
      "y",
      "z",
    ]);
    const positions = [...store.nodes].map(
      ([id, p]) => [id, { ...p.pos }] as const,
    );
    store.replaceSnapshot(
      {
        ...initial,
        processes: [
          ...initial.processes,
          process({ pid: 103, start_time_ticks: 3 }, "new"),
        ],
      },
      false,
      1000,
    );
    for (const [id, pos] of positions)
      assert.deepEqual(
        store.nodes.get(id)!.pos,
        pos,
        "structural additions preserve positions",
      );

    const signal = {
      timestamp_ns: 123,
      src_pid: a.pid,
      dst_pid: b.pid,
      source_id: a,
      destination_id: b,
      signal: 10,
    };
    assert.deepEqual(
      store.ingestActivity(spaceActivity({ signals: [signal] })).signals,
      [signal],
    );
    assert.deepEqual(
      store.ingestActivity(
        spaceActivity({ signals: [signal] }),
        new Set([key(a)]),
      ).signals,
      [],
    );
    assert.deepEqual(
      store.ingestActivity(
        spaceActivity({
          signals: [
            { ...signal, destination_id: { ...b, start_time_ticks: 999 } },
          ],
        }),
      ).signals,
      [],
    );
    assert.equal(
      store.ingestActivity(spaceActivity({ signals: Array(1000).fill(signal) }))
        .signals!.length,
      256,
    );
    assert.deepEqual(store.ingestActivity(spaceActivity()).signals, []);
    const file = {
      process_id: a,
      file: { device: { major: 8, minor: 1 }, inode: "9", generation: 0 },
      path: "/tmp/data.txt",
      bytes: 10,
      count: 1,
      write: true,
    };
    const update = store.ingestActivity(
      spaceActivity({
        window_ms: 100,
        cpu: [
          cpuActivity({ process_id: a, runtime_ns: 100, running_threads: 1 }),
        ],
        files: [file],
        ipc: [
          { ...endpoint(a), write: true, bytes: 4, count: 1 },
          { ...endpoint(b), write: false, bytes: 4, count: 1 },
        ],
      }),
      new Set(store.nodes.keys()),
      1000,
    );
    assert.equal(update.filesChanged, true);
    assert.equal(
      update.routes.filter((r) => r.kind === "connection").length,
      2,
    );
    assert.ok(
      update.routes
        .filter((r) => r.kind === "connection")
        .every((r) => r.direction === 1),
    );
    assert.deepEqual(store.edgeStats.get("pipe"), {
      bytes: 8,
      count: 2,
      time: 1000,
    });
    assert.equal(store.cpuGlows.get(key(a))!.last, 1000);
    assert.ok(store.filePositions.has(fileKey(file)));
    const filtered = store.ingestActivity(
      spaceActivity({
        ipc: [{ ...endpoint(a), write: true, bytes: 2, count: 1 }],
      }),
      new Set([key(a)]),
      1100,
    );
    assert.deepEqual(
      filtered.routes.map((r) => r.kind),
      ["port"],
      "a hidden peer cannot be used as an exact visible route",
    );
    assert.equal(
      store.edgeStats.get("pipe")!.time,
      1000,
      "filtered activity does not replace visible relation totals",
    );
    const ambiguous = {
      ...relation,
      id: "other",
      peer: endpoint({ pid: 103, start_time_ticks: 3 }),
    };
    store.replaceSnapshot(
      { ...store.snapshot, fd_relations: [relation, ambiguous] },
      false,
      1200,
    );
    assert.deepEqual(
      store
        .ingestActivity(
          spaceActivity({
            ipc: [{ ...endpoint(a), write: true, bytes: 1, count: 1 }],
          }),
          new Set(store.nodes.keys()),
          1200,
        )
        .routes.map((r) => r.kind),
      ["port"],
      "multiple peers retain actor-only activity",
    );
    assert.equal(
      store.pruneFiles(31000),
      true,
      "retained files expire at 30 seconds",
    );
    assert.equal(store.filePositions.size, 0);
    store.replaceSnapshot(
      {
        processes: [process({ ...a, start_time_ticks: 99 }, "reused")],
        fd_relations: [],
      },
      false,
      32000,
    );
    assert.equal(store.cpuGlows.has(key(a)), false);
    assert.equal(store.edgeStats.size, 0);
    store.ingestActivity(
      spaceActivity({
        files: [file],
        cpu: [cpuActivity({ process_id: a, runtime_ns: 10 })],
      }),
      new Set(store.nodes.keys()),
      32000,
    );
    assert.equal(
      store.recentFiles.entries.size,
      0,
      "old process identities cannot create activity",
    );
    assert.equal(store.cpuGlows.has(key(a)), false);

    const sources: TestEventSource[] = [],
      seen: [string, ...unknown[]][] = [];
    const live = new SpaceDataStore(
      {
        snapshot() {
          seen.push(["snapshot", live.snapshot.sequence]);
        },
        activity(update) {
          seen.push(["activity", update.routes.length]);
        },
        reset() {
          seen.push(["reset"]);
        },
        gap() {
          seen.push(["gap"]);
        },
        status(message) {
          seen.push(["status", message]);
        },
      },
      undefined,
      (url) => {
        const source = new TestEventSource(url);
        sources.push(source);
        return source.asEventSource();
      },
    );
    assert.equal(
      sources.length,
      0,
      "importing and constructing the data store starts no browser resources",
    );
    live.start();
    live.start();
    assert.equal(sources.length, 1, "start is idempotent");
    assert.equal(sources[0].url, "/api/system/events");
    sources[0].open();
    sources[0].emit("snapshot", { ...initial, kind: "full", sequence: 1 });
    assert.equal(live.snapshot.sequence, 1);
    sources[0].emit("activity", { files: [file] });
    assert.equal(live.recentFiles.entries.size, 1);
    sources[0].emit("gap", {});
    assert.equal(
      live.snapshot.sequence,
      1,
      "gap retains the structural baseline until the next full snapshot",
    );
    assert.ok(seen.some(([type]) => type === "gap"));
    sources[0].emit("snapshot", {
      kind: "delta",
      sequence: 2,
      base_sequence: -1,
      processes: [],
      fd_relations: [],
    });
    assert.equal(sources[0].readyState, 2);
    assert.equal(
      sources.length,
      2,
      "invalid baseline reconnects for a full snapshot",
    );
    assert.equal(
      live.recentFiles.entries.size,
      0,
      "reconnect clears transient activity",
    );
    sources[1].emit("snapshot", { ...initial, kind: "full", sequence: 1 });
    live.stop();
    const afterStop = seen.length;
    sources[1].emit("snapshot", {
      kind: "full",
      sequence: 99,
      processes: [],
      fd_relations: [],
    });
    sources[1].error();
    assert.equal(
      seen.length,
      afterStop,
      "closed sources cannot update state or schedule retries",
    );
    live.start();
    assert.equal(sources.length, 3);
    sources[2].error();
    assert.equal(sources[2].readyState, 2);
    live.stop();
    await new Promise((resolve) => setTimeout(resolve, 3100));
    assert.equal(sources.length, 3, "stop cancels an error retry");
    console.log(
      "SPACE data passed: plain coordinates, stable layout, filtering, activity routes, identity reuse, expiry, lazy SSE, resynchronization, stale sources, retry cancellation.",
    );
  });
}
