// Pure process placement: full forest layout and stable incremental updates.

// Exports ProcessLayoutNode, treeLayout, and stableLayout; no SSE or rendering state.
import { key, PROCESS_SIDE } from "./model.js";
import type { ProcessId, FdRelation } from "../shared/api-types.js";
import type { Position, TreePosition } from "./types.js";

/** The identity, parent relationship, and optional name used to place a process. */
export interface ProcessLayoutNode {
  identity: ProcessId;
  parent_id?: ProcessId | null;
  name?: string;
}
interface PackedTree {
  positions: Map<string, TreePosition>;
  width: number;
  height: number;
  /** Largest number of distinct socket peers of any process in this block. */
  socketPeers: number;
}

const WRAPPED_ROW_OFFSET = PROCESS_SIDE;

/**
 * Place a process forest on the x/y plane, centered horizontally.
 * Missing parents and self-parent links become roots; cycles are broken
 * deterministically. Keys include both PID and process start time.
 * Leaf siblings share a block that can wrap onto multiple rows. Sibling
 * branches with descendants occupy separate horizontal ranges. Trees with
 * different roots can wrap as whole units. Depth describes ancestry, not rows.
 * Roots named kthreadd are packed first; other roots retain PID/start-time order.
 * xGap and yGap control horizontal and vertical spacing.
 * Distinct confirmed socket peers prioritize central sibling blocks and leaf
 * slots. Shared descriptors, candidates, external and stale peers do not count.
 */
export function treeLayout(
  processes: ProcessLayoutNode[],
  xGap = 4.8,
  yGap = 6.5,
  relations: readonly FdRelation[] = [],
): Map<string, TreePosition> {
  const ordered = [...processes].sort(
    (a, b) =>
      a.identity.pid - b.identity.pid ||
      a.identity.start_time_ticks - b.identity.start_time_ticks,
  );
  const known = new Map(ordered.map((node) => [key(node.identity), node]));
  const peers = new Map<string, Set<string>>();
  for (const relation of relations) {
    if (
      !relation.peer ||
      relation.shared ||
      relation.candidate ||
      relation.endpoint.resource.kind !== "socket" ||
      relation.peer.resource.kind !== "socket"
    )
      continue;
    const a = key(relation.endpoint.process_id),
      b = key(relation.peer.process_id);
    if (a === b || !known.has(a) || !known.has(b)) continue;
    for (const [owner, peer] of [
      [a, b],
      [b, a],
    ]) {
      if (!peers.has(owner)) peers.set(owner, new Set());
      peers.get(owner)!.add(peer);
    }
  }
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
    .filter((id) => !parents.get(id))
    .sort(
      (a, b) =>
        Number(known.get(b)!.name === "kthreadd") -
          Number(known.get(a)!.name === "kthreadd") ||
        rank.get(a)! - rank.get(b)!,
    );
  const pack = (
    layouts: PackedTree[],
    gap: number,
    wrapRows: boolean,
  ): PackedTree => {
    if (!layouts.length)
      return { positions: new Map(), width: 0, height: 0, socketPeers: 0 };
    // Balance widths around the busiest block so a wide quiet subtree cannot
    // push a socket hub back to the edge. Sibling order breaks degree ties.
    if (!wrapRows && layouts.some((layout) => layout.socketPeers > 1)) {
      const prioritized = layouts
        .map((layout, index) => ({ layout, index }))
        .sort(
          (a, b) =>
            b.layout.socketPeers - a.layout.socketPeers || a.index - b.index,
        );
      const left: PackedTree[] = [],
        right: PackedTree[] = [];
      let leftWidth = 0,
        rightWidth = 0;
      for (const { layout } of prioritized.slice(1)) {
        if (leftWidth <= rightWidth) {
          left.push(layout);
          leftWidth += layout.width + gap;
        } else {
          right.push(layout);
          rightWidth += layout.width + gap;
        }
      }
      layouts = [...left.reverse(), prioritized[0].layout, ...right];
    }
    const area = layouts.reduce(
      (sum, layout) => sum + (layout.width + gap) * (layout.height + gap),
      0,
    );
    const target = wrapRows
      ? Math.max(
          40,
          ...layouts.map((layout) => layout.width),
          Math.sqrt(area) * 1.4,
        )
      : Infinity;
    const positions = new Map<string, TreePosition>();
    let cursorX = 0,
      cursorY = 0,
      rowHeight = 0,
      rowOffset = 0,
      width = 0;
    for (const layout of layouts) {
      if (cursorX > rowOffset && cursorX - rowOffset + layout.width > target) {
        // Shift each wrapped row together with all positions in its blocks.
        rowOffset += WRAPPED_ROW_OFFSET;
        cursorX = rowOffset;
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
    return {
      positions,
      width,
      height: cursorY + rowHeight,
      socketPeers: Math.max(...layouts.map((layout) => layout.socketPeers)),
    };
  };
  const build = (id: string): PackedTree => {
    const childIds = children.get(id)!,
      leaves = childIds.filter((child) => !children.get(child)!.length),
      layouts: PackedTree[] = [];
    // Insert the wrapped leaf block at the first leaf's place in sibling order.
    for (const child of childIds) {
      if (children.get(child)!.length) layouts.push(build(child));
      else if (child === leaves[0]) {
        const block = pack(leaves.map(build), 3, true);
        if (block.socketPeers > 1) {
          const slots = [...block.positions.values()];
          const centerX =
            (Math.min(...slots.map((p) => p.x)) +
              Math.max(...slots.map((p) => p.x))) /
            2;
          const centerY =
            (Math.min(...slots.map((p) => p.y)) +
              Math.max(...slots.map((p) => p.y))) /
            2;
          const distance = (p: TreePosition) =>
            (p.x - centerX) ** 2 + (p.y - centerY) ** 2;
          const available = [...slots].sort(
            (a, b) => distance(a) - distance(b) || a.y - b.y || a.x - b.x,
          );
          const hubs = leaves
            .filter((leaf) => (peers.get(leaf)?.size ?? 0) > 1)
            .sort(
              (a, b) =>
                peers.get(b)!.size - peers.get(a)!.size ||
                rank.get(a)! - rank.get(b)!,
            );
          const placements = new Map<string, TreePosition>();
          const used = new Set<TreePosition>();
          for (const [index, hub] of hubs.entries()) {
            const slot = available[index];
            used.add(slot);
            placements.set(hub, {
              ...block.positions.get(hub)!,
              x: slot.x,
              y: slot.y,
            });
          }
          const remaining = slots.filter((slot) => !used.has(slot));
          let nextSlot = 0;
          for (const leaf of leaves) {
            if (placements.has(leaf)) continue;
            const slot = remaining[nextSlot++];
            placements.set(leaf, {
              ...block.positions.get(leaf)!,
              x: slot.x,
              y: slot.y,
            });
          }
          block.positions = placements;
        }
        layouts.push(block);
      }
    }
    const packed = pack(layouts, 3, false);
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
      socketPeers: Math.max(peers.get(id)?.size ?? 0, packed.socketPeers),
    };
  };
  const forest = pack(roots.map(build), 9, true),
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

/**
 * Preserve x/y coordinates of live identities and place newcomers in vacant
 * space near their parent. Without surviving previous positions, use treeLayout.
 * Parent and depth metadata always describe the current normalized forest.
 * Inputs and previous coordinates are not mutated.
 */
export function stableLayout(
  processes: ProcessLayoutNode[],
  previous: ReadonlyMap<string, Pick<Position, "x" | "y">> = new Map(),
  xGap = 4.8,
  yGap = 6.5,
  relations: readonly FdRelation[] = [],
): Map<string, TreePosition> {
  const initial = treeLayout(processes, xGap, yGap, relations),
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
          const x = origin.x + dx * xGap + dy * WRAPPED_ROW_OFFSET,
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

if (import.meta.vitest) {
  const { test } = import.meta.vitest;
  const processNode = (pid: number, parent: number | null = null) => ({
    identity: { pid, start_time_ticks: pid * 10 },
    parent_id:
      parent !== null ? { pid: parent, start_time_ticks: parent * 10 } : null,
  });

  test("kthreadd root is packed before systemd without changing tree internals", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
    const nodes = [
      { ...processNode(1), name: "systemd" },
      { ...processNode(2), name: "kthreadd" },
      processNode(3, 2),
      processNode(4, 1),
      processNode(5, 2),
      processNode(6, 3),
      processNode(8),
    ];
    const layout = treeLayout(nodes);
    assert.deepEqual(
      [...layout].filter(([, p]) => p.parent === null).map(([id]) => id),
      ["2:20", "1:10", "8:80"],
    );
    assert.ok(layout.get("2:20")!.x < layout.get("1:10")!.x);
    assert.deepEqual(treeLayout([...nodes].reverse()), layout);
    for (const group of [
      [1, 4],
      [2, 3, 5, 6],
    ]) {
      const standalone = treeLayout(
        nodes.filter((node) => group.includes(node.identity.pid)),
      );
      const rootId = key(processNode(group[0]).identity),
        root = layout.get(rootId)!,
        standaloneRoot = standalone.get(rootId)!;
      for (const [id, p] of standalone) {
        const placed = layout.get(id)!;
        assert.ok(
          Math.abs(placed.x - root.x - (p.x - standaloneRoot.x)) < 1e-9,
        );
        assert.equal(placed.y - root.y, p.y - standaloneRoot.y);
        assert.equal(placed.parent, p.parent);
        assert.equal(placed.depth, p.depth);
      }
    }
    const updated = stableLayout([...nodes, processNode(9, 2)], layout);
    for (const [id, p] of layout) assert.deepEqual(updated.get(id), p);
    assert.deepEqual(stableLayout(nodes), layout);
  });

  test("names do not reorder ordinary roots or kthreadd-named children", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
    const nodes = [
      processNode(1),
      processNode(2),
      processNode(3, 1),
      processNode(4, 1),
      processNode(5),
    ];
    const named = nodes.map((node) => ({
      ...node,
      name: node.identity.pid === 4 ? "kthreadd" : "systemd",
    }));
    assert.deepEqual(treeLayout(named), treeLayout(nodes));
  });

  test("leaf siblings wrap while recursive branches stay side by side", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
    const nodes = [processNode(1)];
    const branches: string[][] = [];
    for (let i = 0; i < 12; i++) {
      const child = processNode(i + 2, 1);
      nodes.push(child);
      const branch = [key(child.identity)];
      for (let j = 0; j < i + 5; j++) {
        const grandchild = processNode(1000 + i * 100 + j, i + 2);
        nodes.push(grandchild);
        branch.push(key(grandchild.identity));
        if (j % 3 === 0) {
          const descendant = processNode(
            10000 + i * 100 + j,
            grandchild.identity.pid,
          );
          nodes.push(descendant);
          branch.push(key(descendant.identity));
        }
      }
      branches.push(branch);
    }
    const layout = treeLayout(nodes);
    assert.ok(
      [...layout.values()].some((p) => p.depth === 2 && p.y > 13),
      "leaf siblings can wrap within a recursive branch",
    );
    const bounds = branches.map((ids) => {
      const xs = ids.map((id) => layout.get(id)!.x);
      return { min: Math.min(...xs), max: Math.max(...xs) };
    });
    for (let i = 1; i < bounds.length; i++)
      assert.ok(
        bounds[i - 1].max < bounds[i].min,
        "sibling subtrees occupy disjoint horizontal ranges",
      );
  });

  test("mixed leaf blocks and recursive branches occupy separate horizontal ranges", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
    const leaves = [
      processNode(2, 1),
      ...Array.from({ length: 30 }, (_, i) => processNode(i + 4, 1)),
    ];
    const firstBranch = [
      processNode(3, 1),
      ...Array.from({ length: 15 }, (_, i) => processNode(i + 40, 3)),
    ];
    const secondBranch = [
      processNode(80, 1),
      processNode(81, 80),
      ...Array.from({ length: 15 }, (_, i) => processNode(i + 82, 81)),
    ];
    const nodes = [processNode(1), ...leaves, ...firstBranch, ...secondBranch],
      layout = treeLayout(nodes);
    const bounds = [leaves, firstBranch, secondBranch].map((group) => {
      const places = group.map((node) => layout.get(key(node.identity))!);
      return {
        minX: Math.min(...places.map((p) => p.x)),
        maxX: Math.max(...places.map((p) => p.x)),
        minY: Math.min(...places.map((p) => p.y)),
      };
    });
    assert.ok(
      new Set(leaves.map((node) => layout.get(key(node.identity))!.y)).size > 1,
      "leaf siblings wrap even when they are interleaved with branches",
    );
    for (let i = 1; i < bounds.length; i++) {
      assert.ok(
        bounds[i - 1].maxX < bounds[i].minX,
        "the leaf block and recursive branches stay side by side",
      );
      assert.equal(
        bounds[i].minY,
        bounds[0].minY,
        "blocks start on the same row",
      );
    }
    for (const node of leaves) {
      const place = layout.get(key(node.identity))!;
      assert.equal(place.depth, 1, "wrapping does not change tree depth");
      assert.equal(place.parent, "1:10");
    }
    assert.deepEqual(treeLayout([...nodes].reverse()), layout);
  });

  test("root trees can wrap while preserving internal leaf rows", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
    const trees = Array.from({ length: 20 }, (_, i) => [
      processNode(i * 100 + 1),
      ...Array.from({ length: 15 }, (_, j) =>
        processNode(i * 100 + j + 2, i * 100 + 1),
      ),
    ]);
    const forestNodes = trees.flat();
    const forest = treeLayout(forestNodes);
    const roots = [...forest.values()].filter((p) => p.parent === null);
    assert.ok(
      new Set(roots.map((p) => p.y)).size > 1,
      "different root trees can wrap onto multiple rows",
    );
    for (const nodes of trees) {
      const standalone = treeLayout(nodes),
        rootId = key(nodes[0].identity),
        root = forest.get(rootId)!,
        standaloneRoot = standalone.get(rootId)!;
      assert.ok(
        new Set(
          [...standalone.values()].filter((p) => p.parent).map((p) => p.y),
        ).size > 1,
        "leaf siblings wrap within each tree",
      );
      for (const [id, p] of standalone) {
        const placed = forest.get(id)!;
        assert.ok(
          Math.abs(placed.x - root.x - (p.x - standaloneRoot.x)) < 1e-9,
        );
        assert.ok(
          Math.abs(placed.y - root.y - (p.y - standaloneRoot.y)) < 1e-9,
        );
        assert.equal(placed.depth, p.depth);
        assert.equal(placed.parent, p.parent);
      }
    }
  });

  test("process tree and stable layout", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
    assert.deepEqual(treeLayout([]), new Map());
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
      "large leaf sibling groups wrap instead of becoming one long row",
    );
    assert.ok(Math.max(...fanoutY) > 20, "wrapped leaf siblings use the plane");
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
}
