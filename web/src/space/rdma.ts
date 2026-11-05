// Pure RDMA queue pair grouping, anchors, labels and device placement for SPACE.

import { Display } from "../shared/display.js";
import { key } from "./model.js";
import type {
  RdmaEndpoint,
  RdmaEndpointOwner,
  RdmaPortInfo,
  RdmaRelation,
} from "../shared/api-types.js";
import type { Position, RdmaDevice, RdmaGroup, RdmaLink } from "./types.js";

export const RDMA_DEVICE_PREFIX = "rdma-device:";
const GROUP_PREFIX = "rdma-group:";

/** SMI and GSI are per-port management QPs of ib_core; they carry no connection. */
export function isRdmaRelationVisible(relation: RdmaRelation) {
  const kind = relation.endpoint.qp_type.kind;
  return kind !== "smi" && kind !== "gsi";
}

export const rdmaDeviceId = (device: string, host?: string) =>
  `${RDMA_DEVICE_PREFIX}${host === undefined ? "" : `${host}@`}${device}`;

/** A live owning process anchors a QP to its hull; any other QP to its device. */
export function rdmaAnchor(endpoint: RdmaEndpoint, live: ReadonlySet<string>) {
  const owner = endpoint.owner;
  return owner.kind === "process" && live.has(key(owner.process_id))
    ? key(owner.process_id)
    : rdmaDeviceId(endpoint.device, endpoint.host);
}

export function rdmaOwnerLabel(owner: RdmaEndpointOwner) {
  if (owner.kind === "process") return `PID ${owner.process_id.pid}`;
  if (owner.kind === "kernel") return owner.name;
  return owner.pid === null ? "Unknown owner" : `PID ${owner.pid}`;
}

/** The consumer of a QP; a kernel CM consumer such as rpcrdma is more specific. */
export function rdmaConsumer(relation: RdmaRelation) {
  const cm = relation.cm?.owner;
  return rdmaOwnerLabel(
    cm && cm.kind === "kernel" ? cm : relation.endpoint.owner,
  );
}

/** The remote address of an unpaired QP, or null when only its QPN is known. */
export function rdmaRemoteLabel(relation: RdmaRelation) {
  const destination = relation.cm?.destination;
  if (destination?.kind === "inet")
    return relation.remote_hostname
      ? `${relation.remote_hostname}:${destination.port}`
      : Display.rdmaAddress(destination);
  if (destination?.kind === "ib") return destination.gid;
  return relation.remote_gid;
}

export const rdmaEndpointLabel = (endpoint: RdmaEndpoint) =>
  `${endpoint.device}${endpoint.port === null ? "" : `/${endpoint.port}`} QP ${endpoint.qpn}`;

/**
 * Groups unpaired QPs by anchor, consumer, type and remote address, like network
 * destinations. Paired QPs become links. Devices include every port's device.
 */
export function rdmaModel(
  relations: readonly RdmaRelation[],
  ports: readonly RdmaPortInfo[],
  live: ReadonlySet<string>,
) {
  const groups = new Map<string, RdmaGroup>();
  const links: RdmaLink[] = [];
  const devices = new Map<string, RdmaDevice>();
  const device = (name: string, host?: string) => {
    const id = rdmaDeviceId(name, host);
    let entry = devices.get(id);
    if (!entry) {
      entry = { id, host, device: name, ports: [], label: name };
      devices.set(id, entry);
    }
    return entry;
  };
  for (const port of ports) device(port.device, port.host).ports.push(port);
  for (const relation of relations) {
    if (!isRdmaRelationVisible(relation)) continue;
    const anchor = rdmaAnchor(relation.endpoint, live);
    if (anchor.startsWith(RDMA_DEVICE_PREFIX))
      device(relation.endpoint.device, relation.endpoint.host);
    if (relation.peer) {
      const to = rdmaAnchor(relation.peer, live);
      if (to.startsWith(RDMA_DEVICE_PREFIX))
        device(relation.peer.device, relation.peer.host);
      links.push({ id: relation.id, relation, from: anchor, to });
      continue;
    }
    const type = Display.rdmaQpType(relation.endpoint.qp_type);
    const remote = rdmaRemoteLabel(relation);
    const id =
      GROUP_PREFIX +
      JSON.stringify([anchor, rdmaConsumer(relation), type, remote]);
    let group = groups.get(id);
    if (!group) {
      group = {
        id,
        anchor,
        device: anchor.startsWith(RDMA_DEVICE_PREFIX)
          ? relation.endpoint.device
          : null,
        consumer: rdmaConsumer(relation),
        remote,
        members: [],
        label: "",
      };
      groups.set(id, group);
    }
    group.members.push(relation);
  }
  for (const group of groups.values()) {
    group.members.sort(
      (a, b) => a.endpoint.qpn - b.endpoint.qpn || a.id.localeCompare(b.id),
    );
    const type = Display.rdmaQpType(group.members[0].endpoint.qp_type);
    const text = `RDMA ${type}${group.remote ? ` ${group.remote}` : ""} ×${group.members.length}`;
    group.label = group.device ? `${group.consumer} · ${text}` : text;
  }
  for (const entry of devices.values())
    entry.ports.sort((a, b) => a.port - b.port);
  return {
    groups,
    links,
    devices: new Map([...devices].sort(([a], [b]) => a.localeCompare(b))),
  };
}

/**
 * Places device anchors (by ID) in a column right of the process layout, on the
 * layer at height `z`. Existing anchors keep their positions until the layout is
 * rearranged.
 */
export function rdmaDeviceLayout(
  devices: Iterable<string>,
  processes: Iterable<Pick<Position, "x" | "y">>,
  previous: ReadonlyMap<string, Position> = new Map(),
  z = 0,
) {
  let maxX = -Infinity,
    minY = Infinity;
  for (const p of processes) {
    maxX = Math.max(maxX, p.x);
    minY = Math.min(minY, p.y);
  }
  if (!Number.isFinite(maxX)) maxX = minY = 0;
  const result = new Map<string, Position>();
  const occupied = new Set<string>();
  const ids = [...devices];
  for (const id of ids) {
    const p = previous.get(id);
    if (p) {
      result.set(id, p);
      occupied.add(`${p.x}:${p.y}`);
    }
  }
  const x = Math.round(maxX / 3) * 3 + 9;
  for (const id of ids) {
    if (result.has(id)) continue;
    for (let slot = 0; ; slot++) {
      const p = { x, y: Math.round(minY / 3) * 3 + slot * 6, z };
      if (occupied.has(`${p.x}:${p.y}`)) continue;
      result.set(id, p);
      occupied.add(`${p.x}:${p.y}`);
      break;
    }
  }
  return result;
}
