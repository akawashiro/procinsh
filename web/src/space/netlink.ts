// Track current kernel-facing Netlink sockets and traffic totals per owner.

import { Display } from "../shared/display.js";
import type {
  FdEndpoint,
  FdRelation,
  IoActivity,
  SocketEndpoint,
} from "../shared/api-types.js";
import { ipcKey, key, socketOwnerKey } from "./model.js";
import type { NetlinkSocketStats, NetlinkSocketInfo } from "./types.js";

/** USERSOCK retains ordinary socket rendering; other families are unaffected. */
export function isKernelNetlink(socket: SocketEndpoint | null | undefined) {
  const protocol = socket?.protocol;
  return (
    protocol?.kind === "other" &&
    protocol.family === 16 &&
    protocol.protocol !== 2
  );
}

const ownerKey = socketOwnerKey;

/** Shared ownership is not a communication peer. Each owner gets its own entry. */
function socketOwners(
  relations: readonly FdRelation[],
  live: ReadonlySet<string>,
) {
  const resources = new Map<
    string,
    { socket: SocketEndpoint; endpoints: Map<string, FdEndpoint> }
  >();
  for (const relation of relations) {
    if (!isKernelNetlink(relation.socket)) continue;
    const resource = ipcKey(relation.endpoint.resource);
    let group = resources.get(resource);
    if (!group) {
      group = { socket: relation.socket!, endpoints: new Map() };
      resources.set(resource, group);
    }
    for (const endpoint of [
      relation.endpoint,
      relation.shared ? relation.peer : null,
    ]) {
      if (
        !endpoint ||
        !live.has(key(endpoint.process_id)) ||
        ipcKey(endpoint.resource) !== resource
      )
        continue;
      group.endpoints.set(
        JSON.stringify([key(endpoint.process_id), endpoint.fd]),
        endpoint,
      );
    }
  }
  const owners = new Map<string, NetlinkSocketInfo>();
  for (const { socket, endpoints } of resources.values()) {
    const holders = [...endpoints.values()].sort(
      (a, b) =>
        a.process_id.pid - b.process_id.pid ||
        a.process_id.start_time_ticks - b.process_id.start_time_ticks ||
        a.fd - b.fd,
    );
    for (const endpoint of holders) {
      const id = ownerKey(endpoint);
      if (owners.has(id)) continue;
      const owned = holders.filter(
        (e) => key(e.process_id) === key(endpoint.process_id),
      );
      owners.set(id, {
        id,
        endpoint: {
          ...endpoint,
          fd_count: owned.reduce((n, e) => n + e.fd_count, 0),
        },
        socket,
        holders,
        label: `${Display.protocol(socket.protocol)} · FD ${endpoint.fd}`,
      });
    }
  }
  return owners;
}

/** Current snapshot sockets are visible even before any traffic is observed. */
export class NetlinkSocketStore {
  readonly entries = new Map<string, NetlinkSocketStats>();
  readonly currentIds = new Set<string>();

  clear() {
    this.entries.clear();
    this.currentIds.clear();
  }

  /** Preserve counters and keep absent owners only while their animations need them. */
  sync(
    relations: readonly FdRelation[],
    live: ReadonlySet<string>,
    retained: ReadonlySet<string> = new Set(),
  ) {
    const owners = socketOwners(relations, live);
    this.currentIds.clear();
    for (const id of owners.keys()) this.currentIds.add(id);
    for (const id of this.entries.keys()) {
      if (!owners.has(id) && !retained.has(id)) this.entries.delete(id);
    }
    for (const [id, owner] of owners) {
      const entry = this.entries.get(id);
      if (entry) Object.assign(entry, owner);
      else
        this.entries.set(id, {
          ...owner,
          readBytes: 0,
          writeBytes: 0,
          readCount: 0,
          writeCount: 0,
        });
    }
  }

  /** Events only update current sockets; they never create a plate. */
  ingest(event: IoActivity, live: ReadonlySet<string>) {
    const entry = this.entries.get(ownerKey(event));
    if (!entry || !this.currentIds.has(entry.id)) return null;
    if (
      !live.has(key(event.process_id)) ||
      !(event.bytes > 0) ||
      !(event.count > 0)
    ) {
      return { id: entry.id, observed: false };
    }
    entry[event.write ? "writeBytes" : "readBytes"] += event.bytes;
    entry[event.write ? "writeCount" : "readCount"] += event.count;
    return { id: entry.id, observed: true };
  }
}
