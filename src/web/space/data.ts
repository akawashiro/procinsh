// System SSE, observed state, retained activity, and plain layout coordinates.
import { Display } from "../shared/display.js";
import type {
  ProcessId,
  Process,
  SpaceActivity,
  SocketEndpoint,
  FdRelation,
  FdEndpoint,
  IoActivity,
  FileActivity,
  FileIdentity,
  IpcIdentity,
  MemoryMap,
  SystemSnapshot,
  SystemSnapshotUpdate,
  CpuActivity,
} from "../shared/api-types.js";
export interface Position {
  x: number;
  y: number;
  z: number;
}
export interface TreePosition {
  x: number;
  y: number;
  depth: number;
  parent: string | null;
}
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
export interface NetworkGroup {
  id: string;
  endpoint: FdEndpoint;
  socket: SocketEndpoint;
  members: FdRelation[];
  label: string;
}
export interface RecentFile {
  id: string;
  process_id: ProcessId;
  file: FileIdentity;
  path: string | null;
  readBytes: number;
  writeBytes: number;
  readCount: number;
  writeCount: number;
  label: string;
  last: number;
}
export type CpuGlow = CpuActivity & { last: number; window_ms: number };
export type Region = MemoryMap & { z: number; h: number };
export const remoteLabel = (socket: SocketEndpoint | null | undefined) =>
  socket?.remote_hostname && socket.remote
    ? `${socket.remote_hostname}:${socket.remote.port}`
    : Display.address(socket?.remote);
export const key = (id: ProcessId) => `${id.pid}:${id.start_time_ticks}`;
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
    group.label = `${Display.protocol(group.socket.protocol)} ${remoteLabel(group.socket)} ×${group.members.length}`;
  }
  return groups;
}

export function networkLayout(
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

export function connectionState(
  e: Pick<FdRelation, "shared" | "candidate" | "peer" | "socket">,
) {
  if (e.shared) return "Shared FD";
  if (e.candidate) return "Candidate peer";
  if (e.peer) return "Confirmed process connection";
  if (e.socket?.network_peer) return "Network destination";
  if (e.socket?.state.kind === "listen") return "Listening";
  if (e.socket?.protocol.kind === "udp") return "No destination set";
  return "Unknown destination";
}
export function layoutMaps<M extends Pick<MemoryMap, "start" | "end">>(
  maps: M[],
) {
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
export function edgeDirection(
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

export function treeLayout(processes: TreeNode[], xGap = 4.8, yGap = 6.5) {
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
export function stableLayout(
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

export function userColor(uid: number | null | undefined) {
  if (uid === null || uid === undefined) return "#889299";
  let hash = Number(uid) >>> 0;
  hash = Math.imul(hash ^ (hash >>> 16), 0x45d9f3b);
  hash = Math.imul(hash ^ (hash >>> 16), 0x45d9f3b);
  const hue = ((hash ^ (hash >>> 16)) >>> 0) % 360;
  return `hsl(${hue}, 65%, 65%)`;
}
export const processColors = (n: {
  uid?: number | null;
  euid?: number | null;
}) => ({ real: userColor(n.uid), effective: userColor(n.euid) });

export const ipcKey = (r: IpcIdentity) =>
  JSON.stringify([r.kind, r.device.major, r.device.minor, r.inode]);
export const ipcLabel = (r: IpcIdentity) =>
  `${r.kind}:${r.device.major}:${r.device.minor}:${r.inode}`;
export const fileLabel = (r: FileIdentity) =>
  `file:${r.device.major}:${r.device.minor}:${r.inode}:${r.generation}`;

// Recent file activity is retained across structural snapshot updates.
export const fileKey = (event: Pick<FileActivity, "process_id" | "file">) =>
  JSON.stringify([
    key(event.process_id),
    event.file.device.major,
    event.file.device.minor,
    event.file.inode,
    event.file.generation,
  ]);
export class RecentFiles {
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
export function fileLayout(
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
export function mergeSnapshot(
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

/** A process with layout coordinates, independent of Three.js objects. */
export interface PlacedProcess extends Process {
  pos: Position;
  regions: Region[];
}
export interface EdgeStat {
  bytes: number;
  count: number;
  time: number;
}
export type ActivityRoute = {
  kind: "file" | "connection" | "port";
  id: string;
  direction?: number | null;
  count: number;
};
export interface ActivityUpdate {
  now: number;
  filesChanged: boolean;
  routes: ActivityRoute[];
}
export interface DataEvents {
  snapshot(): void;
  activity(update: ActivityUpdate): void;
  reset(): void;
  gap(): void;
  status(message: string | null): void;
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
