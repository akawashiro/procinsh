// Build host floors into a caller-owned group and return their label anchors.

import * as T from "three";
import type { SceneInput } from "./contracts.js";
import { addressHost } from "./hosts.js";
import type { RenderNode, HostLayerView } from "./types.js";

/** Translucent floors under each host of the merged view, with label anchors. */
export function buildLayers(
  input: SceneInput,
  nodes: ReadonlyMap<string, RenderNode>,
  geometryGroup: T.Group,
) {
  const dummy = new T.Object3D();
  const layerViews: HostLayerView[] = [];
  const hosts = new Map<string, T.Vector3[]>();
  for (const n of nodes.values())
    if (n.identity.host !== undefined)
      hosts.set(n.identity.host, [
        ...(hosts.get(n.identity.host) ?? []),
        n.pos,
      ]);
  for (const [address, p] of input.hardware?.positions ?? []) {
    const host = addressHost(address);
    if (host !== undefined && hosts.has(host))
      hosts.get(host)!.push(new T.Vector3(p.x, p.y, p.z));
  }
  for (const [id, p] of input.rdma?.devicePositions ?? []) {
    const host = input.rdma?.devices.get(id)?.host;
    if (host !== undefined && hosts.has(host))
      hosts.get(host)!.push(new T.Vector3(p.x, p.y, p.z));
  }
  if (!hosts.size) return layerViews;
  const floors = new T.InstancedMesh(
    new T.PlaneGeometry(1, 1),
    new T.MeshBasicMaterial({
      color: 0x4bd6bf,
      transparent: true,
      opacity: 0.045,
      side: T.DoubleSide,
      depthWrite: false,
    }),
    hosts.size,
  );
  floors.name = "host-layers";
  let index = 0;
  for (const [host, points] of hosts) {
    const box = new T.Box3().setFromPoints(points).expandByScalar(4);
    const z = Math.min(...points.map((p) => p.z)) - 0.3;
    dummy.position.set(
      (box.min.x + box.max.x) / 2,
      (box.min.y + box.max.y) / 2,
      z,
    );
    dummy.scale.set(box.max.x - box.min.x, box.max.y - box.min.y, 1);
    dummy.updateMatrix();
    floors.setMatrixAt(index++, dummy.matrix);
    layerViews.push({
      host,
      label: input.hostLabels?.get(host) ?? host,
      pos: new T.Vector3(box.min.x, box.min.y, z),
    });
  }
  dummy.scale.set(1, 1, 1);
  geometryGroup.add(floors);
  return layerViews;
}
