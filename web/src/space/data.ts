// System SSE, observed state, retained activity, and plain layout coordinates.
import { Display } from "../shared/display.js";
import { key, remoteLabel, ipcKey, fileLabel } from "./model.js";
import type {
  Position,
  TreePosition,
  NetworkGroup,
  RecentFile,
  CpuGlow,
  PlacedProcess,
  EdgeStat,
  ActivityRoute,
  ActivityUpdate,
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

/** @inline */
interface TreeNode {
  identity: ProcessId;
  parent_id?: ProcessId | null;
}
interface PackedTree {
  positions: Map<string, TreePosition>;
  width: number;
  height: number;
}

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

function layoutMaps<M extends Pick<MemoryMap, "start" | "end">>(maps: M[]) {
  const sorted = [...maps].sort((a, b) =>
    BigInt(a.start) < BigInt(b.start) ? -1 : 1,
  );
  const sizes = sorted.map((m) => Number(BigInt(m.end) - BigInt(m.start)));
  const total = sizes.reduce((a, b) => a + b, 0) || 1,
    gap = 0.018;
  let z = 0;
  return sorted
    .map((m, i) => {
      const h = Math.max(0.008, (sizes[i] / total) * 7);
      const result = { ...m, z, h };
      z += h + gap;
      return result;
    })
    .map((m, _, all) => ({ ...m, z: (m.z / z) * 8, h: (m.h / z) * 8 }));
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

function treeLayout(processes: TreeNode[], xGap = 4.8, yGap = 6.5) {
  const ordered = [...processes].sort(
    (a, b) =>
      a.identity.pid - b.identity.pid ||
      a.identity.start_time_ticks - b.identity.start_time_ticks,
  );
  const known = new Map(ordered.map((node) => [key(node.identity), node]));
  const rank = new Map(
    ordered.map((node, index) => [key(node.identity), index]),
  );
  const parents = new Map<string, string | null>();
  for (const node of ordered) {
    const id = key(node.identity),
      parent = node.parent_id && key(node.parent_id);
    parents.set(
      id,
      parent && parent !== id && known.has(parent) ? parent : null,
    );
  }

  // A corrupt or racing proc snapshot must not make the layout recurse forever.
  for (const node of ordered) {
    let id: string | null | undefined = key(node.identity);
    const path: string[] = [],
      seen = new Map<string, number>();
    while (id && parents.has(id)) {
      if (seen.has(id)) {
        const cycle = path.slice(seen.get(id));
        const root = cycle.reduce((a, b) =>
          rank.get(a)! < rank.get(b)! ? a : b,
        );
        parents.set(root, null);
        break;
      }
      seen.set(id, path.length);
      path.push(id);
      id = parents.get(id);
    }
  }

  const children = new Map<string, string[]>(
    ordered.map((node) => [key(node.identity), []]),
  );
  for (const [id, parent] of parents)
    if (parent) children.get(parent)!.push(id);
  for (const list of children.values())
    list.sort((a, b) => rank.get(a)! - rank.get(b)!);
  const roots = ordered
    .map((node) => key(node.identity))
    .filter((id) => !parents.get(id));
  const pack = (layouts: PackedTree[], gap: number): PackedTree => {
    if (!layouts.length) return { positions: new Map(), width: 0, height: 0 };
    const area = layouts.reduce(
      (sum, layout) => sum + (layout.width + gap) * (layout.height + gap),
      0,
    );
    const target = Math.max(
      40,
      ...layouts.map((layout) => layout.width),
      Math.sqrt(area) * 1.4,
    );
    const positions = new Map<string, TreePosition>();
    let cursorX = 0,
      cursorY = 0,
      rowHeight = 0,
      width = 0;
    for (const layout of layouts) {
      if (cursorX && cursorX + layout.width > target) {
        cursorX = 0;
        cursorY += rowHeight + gap;
        rowHeight = 0;
      }
      for (const [id, position] of layout.positions)
        positions.set(id, {
          ...position,
          x: position.x + cursorX,
          y: position.y + cursorY,
        });
      cursorX += layout.width + gap;
      width = Math.max(width, cursorX - gap);
      rowHeight = Math.max(rowHeight, layout.height);
    }
    return { positions, width, height: cursorY + rowHeight };
  };
  const build = (id: string): PackedTree => {
    const packed = pack(children.get(id)!.map(build), 3);
    const width = Math.max(xGap, packed.width),
      offsetX = (width - packed.width) / 2;
    const positions = new Map<string, TreePosition>([
      [id, { x: width / 2, y: 0, depth: 0, parent: parents.get(id)! }],
    ]);
    for (const [child, position] of packed.positions)
      positions.set(child, {
        ...position,
        x: position.x + offsetX,
        y: position.y + yGap,
        depth: position.depth + 1,
      });
    return {
      positions,
      width,
      height: packed.height ? packed.height + yGap : yGap,
    };
  };
  const forest = pack(roots.map(build), 9),
    result = forest.positions;
  const values = [...result.values()];
  const center = values.length
    ? (Math.min(...values.map((v) => v.x)) +
        Math.max(...values.map((v) => v.x))) /
      2
    : 0;
  for (const value of values) value.x -= center;
  return result;
}

// Keep live identities anchored; only newcomers consume vacant space.
function stableLayout(
  processes: TreeNode[],
  previous: ReadonlyMap<string, Pick<Position, "x" | "y">> = new Map(),
  xGap = 4.8,
  yGap = 6.5,
) {
  const initial = treeLayout(processes, xGap, yGap),
    result = new Map<string, TreePosition>();
  for (const [id, place] of initial) {
    const old = previous.get(id);
    if (old) result.set(id, { ...place, x: old.x, y: old.y });
  }
  if (!result.size) return initial;
  const buckets = new Map<string, TreePosition[]>(),
    cell = (x: number, y: number) => `${x}:${y}`;
  const occupy = (place: TreePosition) => {
    const id = cell(Math.floor(place.x / xGap), Math.floor(place.y / yGap));
    if (!buckets.has(id)) buckets.set(id, []);
    buckets.get(id)!.push(place);
  };
  const vacant = (x: number, y: number) => {
    const cx = Math.floor(x / xGap),
      cy = Math.floor(y / yGap);
    for (let dx = -1; dx <= 1; dx++)
      for (let dy = -1; dy <= 1; dy++) {
        for (const p of buckets.get(cell(cx + dx, cy + dy)) || []) {
          if (
            Math.abs(p.x - x) < xGap - 1e-9 &&
            Math.abs(p.y - y) < yGap - 1e-9
          )
            return false;
        }
      }
    return true;
  };
  for (const place of result.values()) occupy(place);
  const existing = [...result.values()];
  const outside = {
    x: Math.max(...existing.map((p) => p.x)) + xGap,
    y: Math.min(...existing.map((p) => p.y)),
  };
  // Normalized tree depth also gives a finite, deterministic order for cycles.
  const newcomers = [...initial]
    .filter(([id]) => !result.has(id))
    .sort(
      (a, b) =>
        a[1].depth - b[1].depth ||
        Number(a[0].split(":")[0]) - Number(b[0].split(":")[0]) ||
        a[0].localeCompare(b[0]),
    );
  for (const [id, place] of newcomers) {
    const parent = place.parent ? result.get(place.parent) : undefined;
    const origin = parent ? { x: parent.x, y: parent.y + yGap } : outside;
    let found = null;
    // Expand square rings below the anchor, keeping children below their parent.
    for (let radius = 0; !found; radius++) {
      for (let dy = 0; dy <= radius && !found; dy++)
        for (let dx = -radius; dx <= radius; dx++) {
          if (Math.max(Math.abs(dx), dy) !== radius) continue;
          const x = origin.x + dx * xGap,
            y = origin.y + dy * yGap;
          if (vacant(x, y)) {
            found = { ...place, x, y };
            break;
          }
        }
    }
    result.set(id, found);
    occupy(found);
  }
  return result;
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
  prune(now: number, live: ReadonlySet<string>) {
    let changed = false;
    for (const [id, file] of this.entries)
      if (now - file.last >= 30000 || !live.has(key(file.process_id))) {
        this.entries.delete(id);
        changed = true;
      }
    return changed;
  }
  ingest(events: FileActivity[], now: number, live: ReadonlySet<string>) {
    let changed = this.prune(now, live);
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
  files: ReadonlyMap<string, RecentFile>,
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
  readonly nodes = new Map<string, PlacedProcess>();
  readonly cpuGlows = new Map<string, CpuGlow>();
  readonly edgeStats = new Map<string, EdgeStat>();
  readonly recentFiles = new RecentFiles();
  network = new Map<string, NetworkGroup>();
  networkPositions = new Map<string, Position>();
  filePositions = new Map<string, Position>();
  private source: EventSource | null = null;
  private retry: ReturnType<typeof setTimeout> | undefined;
  private running = false;

  constructor(
    private readonly events: DataEvents = quietEvents,
    private readonly visibleIds: () => ReadonlySet<string> = () =>
      new Set(this.nodes.keys()),
    private readonly openEvents: (url: string) => EventSource = (url) =>
      new EventSource(url),
  ) {}

  replaceSnapshot(
    snapshot: SystemSnapshot,
    rearrange = false,
    now = performance.now(),
  ) {
    this.snapshot = snapshot;
    const live = new Set(snapshot.processes.map((p) => key(p.identity)));
    for (const id of this.nodes.keys())
      if (!live.has(id)) this.cpuGlows.delete(id);
    const edges = new Set(snapshot.fd_relations.map((e) => e.id));
    for (const id of this.edgeStats.keys())
      if (!edges.has(id)) this.edgeStats.delete(id);
    const layout = rearrange
      ? treeLayout(snapshot.processes)
      : stableLayout(snapshot.processes, this.positions());
    this.nodes.clear();
    for (const p of snapshot.processes) {
      const place = layout.get(key(p.identity));
      this.nodes.set(key(p.identity), {
        ...p,
        pos: { x: place?.x ?? 0, y: place?.y ?? 0, z: 0 },
        regions: layoutMaps(p.maps),
      });
    }
    this.recentFiles.prune(now, live);
    this.network = networkGroups(snapshot.fd_relations);
    this.networkPositions = networkLayout(
      this.network,
      this.positions(),
      rearrange ? new Map() : this.networkPositions,
    );
    if (rearrange) this.filePositions.clear();
    this.layoutFiles();
  }
  private positions() {
    return new Map([...this.nodes].map(([id, n]) => [id, n.pos]));
  }
  private layoutFiles() {
    this.filePositions = fileLayout(
      this.recentFiles.entries,
      this.positions(),
      this.filePositions,
    );
  }

  /** Matches the relations included in the scene for the current search. */
  private visibleRelations(visible: ReadonlySet<string>) {
    const grouped = new Set(
      [...this.network.values()].flatMap((g) => g.members.map((e) => e.id)),
    );
    return this.snapshot.fd_relations.filter((e) => {
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
    now = performance.now(),
  ): ActivityUpdate {
    const filesChanged = this.recentFiles.ingest(
      activity.files || [],
      now,
      new Set(this.nodes.keys()),
    );
    if (filesChanged) this.layoutFiles();
    const routes: ActivityRoute[] = [];
    for (const e of activity.files || []) {
      const id = fileKey(e);
      if (
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
      if (this.nodes.has(id))
        this.cpuGlows.set(id, {
          ...e,
          window_ms: activity.window_ms || 100,
          last: now,
        });
    }
    const relations = this.visibleRelations(visible);
    for (const e of activity.ipc || []) {
      const links = relations.filter((r) => edgeDirection(r, e) !== null);
      const exact = links.filter((r) => !r.candidate);
      const candidates = exact.length ? exact : links;
      if (candidates.length !== 1) {
        if (this.nodes.has(key(e.process_id)))
          routes.push({ kind: "port", id: key(e.process_id), count: e.count });
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
        id: relation.id,
        direction: edgeDirection(relation, e),
        count: e.count,
      });
    }
    return { now, filesChanged, routes };
  }
  pruneFiles(now = performance.now()) {
    const changed = this.recentFiles.prune(now, new Set(this.nodes.keys()));
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
      this.events.snapshot();
    });
    current.addEventListener("activity", (event) => {
      if (this.source === current)
        this.events.activity(this.ingestActivity(JSON.parse(event.data)));
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
  test("process tree and stable layout", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
    const processNode = (pid: number, parent: number | null = null) => ({
      identity: { pid, start_time_ticks: pid * 10 },
      parent_id:
        parent !== null ? { pid: parent, start_time_ticks: parent * 10 } : null,
    });
    const tree = treeLayout([
      processNode(1),
      processNode(2, 1),
      processNode(3, 1),
      processNode(4, 2),
      processNode(8, 99),
    ]);
    assert.equal(tree.get("1:10")!.y, 0);
    assert.ok(tree.get("2:20")!.y > tree.get("1:10")!.y);
    assert.ok(tree.get("4:40")!.y > tree.get("2:20")!.y);
    assert.equal(
      tree.get("1:10")!.x,
      (tree.get("2:20")!.x + tree.get("3:30")!.x) / 2,
    );
    assert.equal(tree.get("8:80")!.y, 0, "missing parent becomes a root");
    const cyclic = treeLayout([
      {
        identity: { pid: 10, start_time_ticks: 1 },
        parent_id: { pid: 11, start_time_ticks: 1 },
      },
      {
        identity: { pid: 11, start_time_ticks: 1 },
        parent_id: { pid: 10, start_time_ticks: 1 },
      },
    ]);
    assert.equal(cyclic.size, 2);
    assert.ok([...cyclic.values()].some((v) => v.parent === null));
    assert.deepEqual(
      [
        ...treeLayout([
          processNode(3, 1),
          processNode(1),
          processNode(2, 1),
        ]).entries(),
      ],
      [
        ...treeLayout([
          processNode(1),
          processNode(2, 1),
          processNode(3, 1),
        ]).entries(),
      ],
    );
    const fanout = treeLayout([
      processNode(1),
      ...Array.from({ length: 400 }, (_, i) => processNode(i + 2, 1)),
    ]);
    const fanoutX = [...fanout.values()].map((v) => v.x),
      fanoutY = [...fanout.values()].map((v) => v.y);
    assert.ok(
      Math.max(...fanoutX) - Math.min(...fanoutX) < 400,
      "large sibling groups wrap instead of becoming one long row",
    );
    assert.ok(
      Math.max(...fanoutY) > 20,
      "wrapped sibling groups use the plane",
    );
    const originalNodes = [
      processNode(1),
      processNode(2, 1),
      processNode(3, 1),
    ];
    const anchored = stableLayout(originalNodes);
    assert.deepEqual(anchored, treeLayout(originalNodes));
    const coordinates = (layout: ReadonlyMap<string, TreePosition>) =>
      [...layout].map(([id, p]) => [id, { x: p.x, y: p.y }]).sort();
    assert.deepEqual(
      coordinates(stableLayout([...originalNodes].reverse(), anchored)),
      coordinates(anchored),
    );
    const grownNodes = [
      ...originalNodes,
      processNode(4, 2),
      processNode(5, 1),
      processNode(6),
    ];
    const grown = stableLayout(grownNodes, anchored);
    for (const [id, p] of anchored)
      assert.deepEqual(grown.get(id), p, "existing nodes stay anchored");
    assert.deepEqual(
      coordinates(stableLayout([...grownNodes].reverse(), anchored)),
      coordinates(grown),
    );
    const changed = stableLayout(
      [processNode(2, 99), processNode(3, 2), processNode(4, 2)],
      grown,
    );
    for (const [id, p] of changed)
      assert.deepEqual(
        { x: p.x, y: p.y },
        { x: grown.get(id)!.x, y: grown.get(id)!.y },
      );
    assert.equal(changed.has("1:10"), false, "removed identities are released");
    const reused = stableLayout(
      [
        processNode(1),
        processNode(2, 1),
        {
          identity: { pid: 3, start_time_ticks: 999 },
          parent_id: processNode(1).identity,
        },
      ],
      anchored,
    );
    assert.equal(reused.has("3:30"), false);
    assert.ok(reused.has("3:999"), "PID reuse is a new identity");
    const vacant = stableLayout(
      originalNodes.filter((n) => n.identity.pid !== 5),
      grown,
    );
    const refilled = stableLayout(
      [...originalNodes, processNode(7, 1)],
      vacant,
    );
    assert.deepEqual(
      { x: refilled.get("7:70")!.x, y: refilled.get("7:70")!.y },
      { x: grown.get("5:50")!.x, y: grown.get("5:50")!.y },
      "vacated child position is reusable",
    );
    const large = stableLayout(
      [
        processNode(1),
        ...Array.from({ length: 1000 }, (_, i) => processNode(i + 2, 1)),
      ],
      stableLayout([processNode(1)]),
    );
    const places = [...large.values()];
    for (let i = 0; i < places.length; i++)
      for (let j = i + 1; j < places.length; j++) {
        assert.ok(
          Math.abs(places[i].x - places[j].x) >= 4.8 - 1e-9 ||
            Math.abs(places[i].y - places[j].y) >= 6.5 - 1e-9,
          "new placements do not overlap",
        );
      }
    assert.deepEqual(stableLayout([], grown), new Map());
    const stableCycle = stableLayout(
      [processNode(1, 2), processNode(2, 1), processNode(9)],
      anchored,
    );
    assert.equal(stableCycle.size, 3);
    assert.ok(
      [...stableCycle.values()].every(
        (p) => Number.isFinite(p.x) && Number.isFinite(p.y),
      ),
    );
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
