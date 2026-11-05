// Allocate one stable hull attachment per process/IPC resource identity.

import * as T from "three";
import type { FdEndpoint } from "../shared/api-types.js";
import type { SceneInput } from "./contracts.js";
import { key, PROCESS_SIDE, socketOwnerKey as ipcOwnerKey } from "./model.js";
import { isKernelNetlink } from "./netlink.js";

export const IPC_STEM_LENGTH = 0.16;
export const IPC_PORT_RADIUS = 0.055;
export interface IpcPort {
  id: string;
  position: T.Vector3;
  normal: T.Vector3;
  radiusScale: number;
  connections: Set<string>;
  networks: Set<string>;
}
type PortRequest = {
  id: string;
  process: string;
  preferred: string;
  height: number;
  normal: T.Vector3;
  connections: Set<string>;
  networks: Set<string>;
};
const slots = [{ u: 0, v: 0, level: 0 }];
let nextLevel = 1;

// Refine the face grid without moving slots that have already been allocated.
function slotPosition(index: number) {
  while (slots.length <= index) {
    const denominator = 2 ** nextLevel;
    for (let v = 1 - denominator; v < denominator; v++)
      for (let u = 1 - denominator; u < denominator; u++) {
        if (u % 2 === 0 && v % 2 === 0) continue;
        slots.push({
          u: u / denominator,
          v: v / denominator,
          level: nextLevel,
        });
      }
    nextLevel++;
  }
  return slots[index];
}

/** Retains face slots across snapshots, alias FD changes and relation ordering. */
export class IpcPortAllocator {
  private allocations = new Map<string, { face: string; slot: number }>();

  allocate(input: SceneInput): Map<string, IpcPort> {
    const requests = new Map<string, PortRequest>();
    const request = (
      endpoint: FdEndpoint,
      normal: T.Vector3,
      preferred: string,
      connection: string,
      network?: string,
    ) => {
      const id = ipcOwnerKey(endpoint);
      let port = requests.get(id);
      if (!port) {
        port = {
          id,
          process: key(endpoint.process_id),
          preferred,
          height: endpoint.resource.kind === "socket" ? 4 : 1.4,
          normal,
          connections: new Set(),
          networks: new Set(),
        };
        requests.set(id, port);
      } else if (preferred < port.preferred) {
        port.preferred = preferred;
        port.normal = normal;
      }
      port.connections.add(connection);
      if (network) port.networks.add(network);
    };
    const grouped = new Set<string>();
    for (const group of input.network.values()) {
      for (const member of group.members) grouped.add(member.id);
      if (
        !input.visible.has(key(group.endpoint.process_id)) ||
        !input.networkPositions.has(group.id)
      )
        continue;
      for (const member of group.members)
        request(
          member.endpoint,
          new T.Vector3(0, 0, 1),
          "0:roof",
          member.id,
          group.id,
        );
    }
    for (const relation of input.snapshot.fd_relations) {
      if (grouped.has(relation.id) || isKernelNetlink(relation.socket))
        continue;
      const a = input.nodes.get(key(relation.endpoint.process_id));
      const b = relation.peer && input.nodes.get(key(relation.peer.process_id));
      if (
        !a ||
        !input.visible.has(key(a.identity)) ||
        (b && !input.visible.has(key(b.identity)))
      )
        continue;
      const aId = ipcOwnerKey(relation.endpoint);
      const bId = relation.peer && ipcOwnerKey(relation.peer);
      const dx = b ? b.pos.x - a.pos.x : 1;
      const dy = b ? b.pos.y - a.pos.y : 0;
      const normal =
        Math.abs(dx) >= Math.abs(dy)
          ? new T.Vector3(Math.sign(dx) || (bId && aId > bId ? -1 : 1), 0, 0)
          : new T.Vector3(0, Math.sign(dy), 0);
      request(
        relation.endpoint,
        normal,
        b ? `1:${bId}` : "2:unobserved",
        relation.id,
      );
      if (b && relation.peer)
        request(
          relation.peer,
          aId === bId ? normal : normal.clone().negate(),
          `1:${aId}`,
          relation.id,
        );
    }
    const faceId = (port: PortRequest) =>
      `${port.process}:${port.height}:${port.normal.toArray()}`;
    for (const [id, allocation] of this.allocations) {
      const port = requests.get(id);
      if (!port || allocation.face !== faceId(port))
        this.allocations.delete(id);
    }
    const occupied = new Map<string, Set<number>>();
    for (const allocation of this.allocations.values()) {
      const used = occupied.get(allocation.face) ?? new Set<number>();
      used.add(allocation.slot);
      occupied.set(allocation.face, used);
    }
    const nextSlot = new Map<string, number>();
    for (const port of [...requests.values()].sort((a, b) =>
      a.id < b.id ? -1 : a.id > b.id ? 1 : 0,
    )) {
      if (this.allocations.has(port.id)) continue;
      const face = faceId(port);
      const used = occupied.get(face) ?? new Set<number>();
      let slot = nextSlot.get(face) ?? 0;
      while (used.has(slot)) slot++;
      used.add(slot);
      nextSlot.set(face, slot + 1);
      occupied.set(face, used);
      this.allocations.set(port.id, { face, slot });
    }
    const levels = new Map<string, number>();
    for (const allocation of this.allocations.values())
      levels.set(
        allocation.face,
        Math.max(
          levels.get(allocation.face) ?? 0,
          slotPosition(allocation.slot).level,
        ),
      );
    const ports = new Map<string, IpcPort>();
    for (const port of requests.values()) {
      const allocation = this.allocations.get(port.id)!;
      const { u, v } = slotPosition(allocation.slot);
      const owner = input.nodes.get(port.process)!;
      const roof = port.normal.z === 1;
      const span = PROCESS_SIDE * 0.4;
      const position = new T.Vector3(
        owner.pos.x,
        owner.pos.y,
        owner.pos.z + (roof ? 8 + IPC_STEM_LENGTH : port.height),
      );
      if (roof) position.add(new T.Vector3(u * span, v * span, 0));
      else
        position
          .addScaledVector(port.normal, PROCESS_SIDE / 2 + IPC_STEM_LENGTH)
          .add(
            new T.Vector3(
              Math.abs(port.normal.y) * u * span,
              Math.abs(port.normal.x) * u * span,
              v * 0.6,
            ),
          );
      const spacing = (roof ? span : 0.6) / 2 ** levels.get(allocation.face)!;
      ports.set(port.id, {
        id: port.id,
        position,
        normal: port.normal,
        radiusScale: Math.min(1, (spacing * 0.4) / IPC_PORT_RADIUS),
        connections: port.connections,
        networks: port.networks,
      });
    }
    return ports;
  }
}
