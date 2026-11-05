// Pure multi-host helpers: qualify host-local identities and compose one snapshot.

import type {
  CrossHostLinks,
  DeviceHandle,
  FdRelation,
  ProcessId,
  RdmaEndpoint,
  RdmaEndpointOwner,
  RdmaRelation,
  SpaceActivity,
  SystemSnapshot,
} from "../shared/api-types.js";
export { ALL_HOSTS } from "../shared/hosts.js";

/** Vertical distance between host layers; one host's markers and files fit inside. */
export const HOST_LAYER_GAP = 56;

const id = (host: string, value: ProcessId): ProcessId => ({ ...value, host });
/** A host-local ID, such as a relation ID or a PCI address, made unique across hosts. */
export const hostId = (host: string, value: string) => `${host}@${value}`;

function rdmaOwner(host: string, owner: RdmaEndpointOwner): RdmaEndpointOwner {
  return owner.kind === "process"
    ? { ...owner, process_id: id(host, owner.process_id) }
    : owner;
}
function rdmaEndpoint(host: string, endpoint: RdmaEndpoint): RdmaEndpoint {
  return { ...endpoint, host, owner: rdmaOwner(host, endpoint.owner) };
}

/**
 * Adds `host` to every process identity and prefixes relation IDs and PCI
 * addresses, so equal identities of different machines stay apart.
 */
export function qualifySnapshot(
  host: string,
  snapshot: SystemSnapshot,
): SystemSnapshot {
  const address = (value: string) => hostId(host, value);
  return {
    ...snapshot,
    processes: snapshot.processes.map((p) => ({
      ...p,
      identity: id(host, p.identity),
      parent_id: p.parent_id && id(host, p.parent_id),
    })),
    fd_relations: snapshot.fd_relations.map((r) => ({
      ...r,
      id: hostId(host, r.id),
      endpoint: { ...r.endpoint, process_id: id(host, r.endpoint.process_id) },
      peer: r.peer && { ...r.peer, process_id: id(host, r.peer.process_id) },
    })),
    rdma_relations: snapshot.rdma_relations?.map((r) => ({
      ...r,
      id: hostId(host, r.id),
      endpoint: rdmaEndpoint(host, r.endpoint),
      peer: r.peer && rdmaEndpoint(host, r.peer),
      cm: r.cm && { ...r.cm, owner: rdmaOwner(host, r.cm.owner) },
    })),
    rdma_ports: snapshot.rdma_ports?.map((p) => ({ ...p, host })),
    hardware_devices: snapshot.hardware_devices?.map((d) => ({
      ...d,
      address: address(d.address),
      parent: d.parent && address(d.parent),
    })),
    hardware_paths: snapshot.hardware_paths?.map((p) => ({
      ...p,
      id: address(p.id),
      a: address(p.a),
      b: address(p.b),
      via: p.via.map(address),
    })),
    device_handles: snapshot.device_handles?.map((h): DeviceHandle => ({
      ...h,
      id: address(h.id),
      device: address(h.device),
      process_id: id(host, h.process_id),
    })),
  };
}

export function qualifyActivity(
  host: string,
  activity: SpaceActivity,
): SpaceActivity {
  return {
    ...activity,
    cpu: activity.cpu?.map((e) => ({
      ...e,
      process_id: id(host, e.process_id),
    })),
    ipc: activity.ipc?.map((e) => ({
      ...e,
      process_id: id(host, e.process_id),
    })),
    files: activity.files?.map((e) => ({
      ...e,
      process_id: id(host, e.process_id),
    })),
    signals: activity.signals?.map((e) => ({
      ...e,
      source_id: id(host, e.source_id),
      destination_id: id(host, e.destination_id),
    })),
  };
}

/**
 * Concatenates qualified host snapshots in layer order. Each cross-host link
 * replaces its two external relations with one relation whose peer is on the
 * other host; links to relations not yet received are skipped.
 */
export function composeHosts(
  snapshots: ReadonlyMap<string, SystemSnapshot>,
  links: CrossHostLinks,
): SystemSnapshot {
  const all = [...snapshots.values()];
  const fd = new Map<string, FdRelation>(
    all.flatMap((s) => s.fd_relations.map((r) => [r.id, r] as const)),
  );
  const rdma = new Map<string, RdmaRelation>(
    all.flatMap((s) => (s.rdma_relations ?? []).map((r) => [r.id, r] as const)),
  );
  const joined: FdRelation[] = [];
  const joinedRdma: RdmaRelation[] = [];
  for (const link of links.rdma ?? []) {
    const [a, b] = [link.a, link.b].map((end) =>
      rdma.get(hostId(end.host, end.relation)),
    );
    if (!a || !b) continue;
    rdma.delete(a.id);
    rdma.delete(b.id);
    joinedRdma.push({
      ...a,
      id: `${a.id}|${b.id}`,
      peer: b.endpoint,
      label: `RDMA ${link.a.host} QP ${a.endpoint.qpn} ↔ ${link.b.host} QP ${b.endpoint.qpn}`,
    });
  }
  for (const link of links.sockets) {
    const [a, b] = [link.a, link.b].map((end) =>
      fd.get(hostId(end.host, end.relation)),
    );
    if (!a || !b) continue;
    fd.delete(a.id);
    fd.delete(b.id);
    joined.push({
      ...a,
      id: `${a.id}|${b.id}`,
      peer: b.endpoint,
      label: `${a.label} ↔ ${link.b.host}`,
      candidate: false,
      shared: false,
    });
  }
  return {
    processes: all.flatMap((s) => s.processes),
    fd_relations: [...fd.values(), ...joined],
    rdma_relations: [...rdma.values(), ...joinedRdma],
    rdma_ports: all.flatMap((s) => s.rdma_ports ?? []),
    hardware_devices: all.flatMap((s) => s.hardware_devices ?? []),
    hardware_paths: all.flatMap((s) => s.hardware_paths ?? []),
    device_handles: all.flatMap((s) => s.device_handles ?? []),
  };
}

/** Host layers top to bottom: the given order, then hosts first seen in `snapshot`. */
export function hostLayers(
  order: readonly string[],
  snapshot: Pick<SystemSnapshot, "processes">,
) {
  const hosts = [...order];
  for (const p of snapshot.processes) {
    const host = p.identity.host;
    if (host !== undefined && !hosts.includes(host)) hosts.push(host);
  }
  return new Map(
    hosts.map((host, index) => [host, 0 - index * HOST_LAYER_GAP]),
  );
}

/** The host of a qualified PCI address, or undefined for a single host. */
export function addressHost(address: string) {
  const at = address.indexOf("@");
  return at < 0 ? undefined : address.slice(0, at);
}
