// Sample userspace geometry and activity only when a tour shot is chosen.

import type { RenderView } from "./contracts.js";
import type { CpuGlow, TourScene } from "./types.js";
import { key } from "./model.js";

export function tourScene(
  view: RenderView,
  cpu: ReadonlyMap<string, CpuGlow>,
  now: number,
): TourScene {
  const children = new Map<string, string[]>();
  for (const [id, node] of view.nodes) {
    if (!node.parent_id) continue;
    const parent = key(node.parent_id);
    const siblings = children.get(parent) ?? [];
    siblings.push(id);
    children.set(parent, siblings);
  }
  function tree(name: string) {
    const pending = [...view.nodes]
      .filter(([, node]) => node.name === name)
      .map(([id]) => id);
    const descendants = new Set<string>();
    while (pending.length) {
      const id = pending.pop()!;
      if (descendants.has(id)) continue;
      descendants.add(id);
      for (const child of children.get(id) ?? []) pending.push(child);
    }
    return descendants;
  }
  const kernel = tree("kthreadd");
  const systemd = tree("systemd");
  const visible = view.hullIds.filter((id) => !kernel.has(id));
  const preferSystemd = visible.some((id) => systemd.has(id));
  const subjects = new Set(
    visible.filter((id) => !preferSystemd || systemd.has(id)),
  );
  const edges = view.edgeViews.filter(
    ({ e }) =>
      subjects.has(key(e.endpoint.process_id)) &&
      (!e.peer || subjects.has(key(e.peer.process_id))),
  );
  const degrees = new Map<string, number>();
  for (const { e } of edges) {
    for (const endpoint of [e.endpoint, e.peer]) {
      if (!endpoint) continue;
      const id = key(endpoint.process_id);
      degrees.set(id, (degrees.get(id) ?? 0) + 1);
    }
  }
  const curves = [
    ...edges,
    ...view.networkViews.filter(({ group }) =>
      subjects.has(key(group.endpoint.process_id)),
    ),
    ...[...view.fileViews.values()].filter(({ file }) =>
      subjects.has(key(file.process_id)),
    ),
    ...[...view.netlinkViews.values()].filter(({ socket }) =>
      subjects.has(key(socket.endpoint.process_id)),
    ),
  ];
  function sample(ids: Iterable<string>) {
    return [...ids].flatMap((id) => {
      const node = view.nodes.get(id);
      if (!node) return [];
      const glow = cpu.get(id);
      return [
        {
          id,
          position: { x: node.pos.x, y: node.pos.y, z: node.pos.z + 4 },
          activity:
            glow && now >= glow.last && now - glow.last < 5000
              ? glow.runtime_ns / Math.max(1, glow.window_ms * 1e6)
              : 0,
          connections: degrees.get(id) ?? 0,
        },
      ];
    });
  }
  return {
    subjects: sample(subjects),
    connections: curves.map(({ curve }) => curve.getPoints(4)),
  };
}
