// Build RDMA geometry into a caller-owned group and return its rendering references.

import * as T from "three";
import type { SceneInput } from "./contracts.js";
import { PROCESS_SIDE } from "./model.js";
import type {
  RenderNode,
  RdmaView,
  RdmaLinkView,
  RdmaDeviceView,
} from "./types.js";
const RDMA_COLOR = 0xb48cff;
/** Height of an RDMA device plate, which rests on its NIC or the host floor. */
export const RDMA_DEVICE_HEIGHT = 0.5;

/** Device plates, QP markers above their anchors, and links between connected QPs. */
export function buildRdma(
  input: SceneInput,
  nodes: ReadonlyMap<string, RenderNode>,
  geometryGroup: T.Group,
  processHeight: number,
) {
  const dummy = new T.Object3D();
  const rdmaViews: RdmaView[] = [];
  const rdmaLinkViews: RdmaLinkView[] = [];
  const rdmaDeviceViews: RdmaDeviceView[] = [];
  const views = { rdmaViews, rdmaLinkViews, rdmaDeviceViews };
  const rdma = input.rdma;
  if (!rdma) return views;
  // The roof of a visible process or device plate; `floor` gives its base instead.
  const top = (anchor: string, floor = false) => {
    const n = nodes.get(anchor);
    if (n)
      return input.visible.has(anchor)
        ? new T.Vector3(n.pos.x, n.pos.y, n.pos.z + (floor ? 0 : processHeight))
        : null;
    const p = rdma.devicePositions.get(anchor);
    return p
      ? new T.Vector3(p.x, p.y, p.z + (floor ? 0 : RDMA_DEVICE_HEIGHT))
      : null;
  };
  const devices = [...rdma.devices.values()].filter((d) =>
    rdma.devicePositions.has(d.id),
  );
  const plates = new T.InstancedMesh(
    new T.BoxGeometry(PROCESS_SIDE, PROCESS_SIDE, RDMA_DEVICE_HEIGHT),
    new T.MeshBasicMaterial({
      color: RDMA_COLOR,
      transparent: true,
      opacity: 0.18,
      depthWrite: false,
    }),
    Math.max(1, devices.length),
  );
  plates.count = devices.length;
  plates.name = "rdma-devices";
  plates.userData.rdmaDevices = devices.map((d) => d.id);
  geometryGroup.add(plates);
  devices.forEach((device, i) => {
    const p = rdma.devicePositions.get(device.id)!;
    const pos = new T.Vector3(p.x, p.y, p.z + RDMA_DEVICE_HEIGHT / 2);
    dummy.position.copy(pos);
    dummy.scale.set(1, 1, 1);
    dummy.updateMatrix();
    plates.setMatrixAt(i, dummy.matrix);
    rdmaDeviceViews.push({ device, pos });
  });
  const plateEdges = new T.InstancedMesh(
    new T.BoxGeometry(PROCESS_SIDE, PROCESS_SIDE, RDMA_DEVICE_HEIGHT),
    new T.MeshBasicMaterial({
      color: RDMA_COLOR,
      wireframe: true,
      transparent: true,
      opacity: 0.7,
    }),
    Math.max(1, devices.length),
  );
  plateEdges.count = plates.count;
  plateEdges.instanceMatrix = plates.instanceMatrix;
  geometryGroup.add(plateEdges);
  const groups = [...rdma.groups.values()].filter(
    (g) => rdma.positions.has(g.id) && top(g.anchor),
  );
  const markers = new T.InstancedMesh(
    new T.IcosahedronGeometry(0.75),
    new T.MeshBasicMaterial({
      color: RDMA_COLOR,
      transparent: true,
      opacity: 0.2,
    }),
    Math.max(1, groups.length),
  );
  markers.count = groups.length;
  markers.name = "rdma-markers";
  markers.userData.rdma = groups.map((g) => g.id);
  geometryGroup.add(markers);
  const markerEdges = new T.InstancedMesh(
    new T.IcosahedronGeometry(0.75),
    new T.MeshBasicMaterial({
      color: 0xd2bbff,
      wireframe: true,
      transparent: true,
      opacity: 0.9,
    }),
    Math.max(1, groups.length),
  );
  markerEdges.count = markers.count;
  markerEdges.instanceMatrix = markers.instanceMatrix;
  geometryGroup.add(markerEdges);
  const positions: number[] = [],
    paths: string[] = [];
  const addCurve = (curve: T.Curve<T.Vector3>, id: string) => {
    const pts = curve.getPoints(24);
    for (let j = 0; j < pts.length - 1; j++) {
      positions.push(...pts[j].toArray(), ...pts[j + 1].toArray());
      paths.push(id);
    }
  };
  groups.forEach((group, i) => {
    const p = rdma.positions.get(group.id)!,
      pos = new T.Vector3(p.x, p.y, p.z),
      start = top(group.anchor)!;
    dummy.position.copy(pos);
    dummy.scale.set(1, 1, 1);
    dummy.updateMatrix();
    markers.setMatrixAt(i, dummy.matrix);
    const curve = new T.CubicBezierCurve3(
      start,
      start.clone().add(new T.Vector3(0, 0, 2)),
      start
        .clone()
        .lerp(pos, 0.7)
        .add(new T.Vector3(0, 0, 2)),
      pos,
    );
    addCurve(curve, group.id);
    rdmaViews.push({ group, pos, curve });
  });
  for (const link of rdma.links) {
    let start = top(link.from),
      end = top(link.to);
    if (!start || !end) continue;
    let curve: RdmaLinkView["curve"];
    if (Math.abs(start.z - end.z) > processHeight) {
      // Between host layers: from the upper anchor's base down to the lower roof.
      if (start.z > end.z) start = top(link.from, true)!;
      else end = top(link.to, true)!;
      const down = new T.Vector3(0, 0, Math.sign(end.z - start.z) * 6);
      curve = new T.CubicBezierCurve3(
        start,
        start.clone().add(down),
        end.clone().sub(down),
        end,
      );
    } else {
      const mid = start.clone().lerp(end, 0.5);
      mid.z += Math.min(8, 3 + start.distanceTo(end) * 0.15);
      if (link.from === link.to) mid.x += 3;
      curve = new T.QuadraticBezierCurve3(start, mid, end);
    }
    addCurve(curve, link.id);
    rdmaLinkViews.push({ link, curve });
  }
  const geometry = new T.BufferGeometry();
  geometry.setAttribute("position", new T.Float32BufferAttribute(positions, 3));
  const lines = new T.LineSegments(
    geometry,
    new T.LineBasicMaterial({
      color: RDMA_COLOR,
      transparent: true,
      opacity: 0.8,
      depthWrite: false,
      blending: T.AdditiveBlending,
    }),
  );
  lines.name = "rdma-lines";
  lines.userData.rdmaPaths = paths;
  geometryGroup.add(lines);
  return views;
}
