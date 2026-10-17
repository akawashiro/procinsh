// @vitest-environment jsdom
import { afterEach, test, vi } from "vitest";
import assert from "node:assert/strict";
import * as T from "three";
import { setSpaceClock } from "../src/space/clock.js";
import { SpaceDataStore } from "../src/space/data.js";
import { createSpaceRenderer } from "../src/space/renderer.js";
import { createSpaceScene } from "../src/space/scene.js";
import { SpaceSelection } from "../src/space/selection.js";
import { key, socketOwnerKey } from "../src/space/model.js";
import type { SystemSnapshot, SpaceActivity } from "../src/shared/api-types.js";
import {
  fdEndpoint,
  fdRelation,
  processInfo,
  memoryMap,
  socketEndpoint,
  spaceActivity,
} from "./support/fixtures.js";

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const source = { pid: 101, start_time_ticks: 1 };
const target = { pid: 102, start_time_ticks: 2 };
const signal = {
  timestamp_ns: 123,
  src_pid: 101,
  dst_pid: 102,
  source_id: source,
  destination_id: target,
  signal: 15,
};
const initial: SystemSnapshot = {
  processes: [
    processInfo({ identity: source, name: "source", maps: [memoryMap()] }),
    processInfo({ identity: target, name: "target", parent_id: source }),
    processInfo({ identity: { pid: 103, start_time_ticks: 3 }, name: "idle" }),
  ],
  fd_relations: [],
};

function setup(reduced = false) {
  vi.stubGlobal("matchMedia", () => ({ matches: reduced }));
  let pendingFrame: FrameRequestCallback;
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    pendingFrame = callback;
    return 1;
  });
  vi.stubGlobal("cancelAnimationFrame", () => {});
  vi.spyOn(document, "hidden", "get").mockReturnValue(false);
  const context = {
    setTransform() {},
    clearRect() {},
    strokeText() {},
    fillText() {},
    beginPath() {},
    arc() {},
    fill() {},
    measureText() {
      return { width: 20 };
    },
  };
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(
    (() => context) as unknown as typeof HTMLCanvasElement.prototype.getContext,
  );
  class WebGLRenderer {
    setPixelRatio() {}
    setSize() {}
    setClearColor() {}
    render() {}
    dispose() {}
  }
  const scene = createSpaceScene();
  const selection = new SpaceSelection();
  const data = new SpaceDataStore(
    undefined,
    undefined,
    undefined,
    (now, suppressed) => renderer.activeResources(now, suppressed),
  );
  const rebuild = () => {
    selection.retain({
      processes: new Set(data.nodes.keys()),
      connections: new Set(data.displaySnapshot.fd_relations.map((r) => r.id)),
      networks: new Set(data.network.keys()),
      files: new Set(data.recentFiles.entries.keys()),
      netlink: new Set(data.netlinkSockets.entries.keys()),
    });
    scene.rebuild(
      {
        snapshot: data.displaySnapshot,
        nodes: data.nodes,
        network: data.network,
        networkPositions: data.networkPositions,
        files: data.recentFiles.entries,
        filePositions: data.filePositions,
        netlink: data.netlinkSockets.entries,
        netlinkPositions: data.netlinkPositions,
        visible: new Set(data.nodes.keys()),
      },
      selection.process,
    );
    renderer.retainParticles(
      new Set(data.network.keys()),
      data.suppressedSocketOwners,
    );
  };
  const renderer = createSpaceRenderer({
    graphics: {
      ...T,
      WebGLRenderer: WebGLRenderer as unknown as typeof T.WebGLRenderer,
    },
    canvas: document.createElement("canvas"),
    labelCanvas: document.createElement("canvas"),
    fpsLabel: document.createElement("div"),
    failure: document.createElement("div"),
    scene: scene.root,
    camera: new T.PerspectiveCamera(),
    view: scene.renderView,
    selection: () => selection,
    cpuGlows: data.cpuGlows,
    beforeFrame(now) {
      if (data.pruneRetention(now)) rebuild();
    },
  });
  renderer.resume();
  return {
    data,
    renderer,
    scene,
    selection,
    snapshot(snapshot: SystemSnapshot, now = 1000, rearrange = false) {
      if (rearrange) renderer.clearParticles();
      data.replaceSnapshot(snapshot, rearrange, now);
      rebuild();
    },
    activity(activity: Partial<SpaceActivity>, now = 1000) {
      const update = data.ingestActivity(
        spaceActivity(activity),
        new Set(data.nodes.keys()),
        now,
      );
      if (update.filesChanged || update.netlinkChanged) rebuild();
      renderer.activity(update);
      return update;
    },
    frame(now: number) {
      pendingFrame!(now);
    },
    clear(now: number) {
      renderer.clearParticles();
      data.pruneRetention(now);
      rebuild();
    },
    dispose() {
      renderer.dispose();
      scene.dispose();
    },
  };
}

test.each([false, true])(
  "exited signal endpoints survive flight and pulse (reduced=%s)",
  (reduced) => {
    const h = setup(reduced);
    try {
      h.snapshot(initial);
      const positions = new Map(
        [...h.data.nodes].map(([id, n]) => [id, n.pos]),
      );
      h.selection.choose("process", key(target));
      h.activity({ signals: [signal] });
      const duration = reduced ? 300 : 1700;
      h.snapshot({ processes: [], fd_relations: [] }, 1100);
      assert.equal(h.data.snapshot.processes.length, 0);
      assert.deepEqual([...h.data.nodes.keys()], [key(source), key(target)]);
      for (const [id, n] of h.data.nodes)
        assert.deepEqual(n.pos, positions.get(id));
      assert.equal(h.data.nodes.get(key(source))!.regions.length, 1);
      assert.equal(h.selection.process, key(target));
      assert.equal(
        h.activity({ signals: [signal] }, 1200).signals!.length,
        0,
        "absent identities cannot start another flight",
      );
      h.frame(1000 + duration + 50);
      assert.equal(h.renderer.signalVisuals()[0].arrived, true);
      const glow = h.scene.cpuGlowVisual(key(target));
      assert.ok(glow);
      assert.ok(glow.r > 0.8, "the exited destination pulses on arrival");
      h.frame(1000 + duration + 399);
      assert.equal(h.data.nodes.size, 2);
      h.frame(1000 + duration + 400);
      assert.equal(h.data.nodes.size, 0);
      assert.equal(h.renderer.signalVisuals().length, 0);
      assert.equal(h.selection.process, null);
    } finally {
      h.dispose();
    }
  },
);

test.each(["pipe", "network", "file", "netlink", "port"] as const)(
  "%s retains its owner and path until the last staggered particle",
  (kind) => {
    const h = setup();
    try {
      const endpoint = fdEndpoint({
        process_id: source,
        ...(kind === "network" || kind === "netlink"
          ? {
              kind: "socket",
              resource: {
                kind: "socket",
                device: { major: 0, minor: 1 },
                inode: "10",
              },
            }
          : {}),
      });
      const relation = fdRelation({
        id: kind,
        endpoint,
        peer: kind === "pipe" ? fdEndpoint({ process_id: target }) : null,
        socket:
          kind === "network"
            ? socketEndpoint({
                remote: { ip: "203.0.113.1", port: 443 },
                network_peer: true,
              })
            : kind === "netlink"
              ? socketEndpoint({
                  protocol: {
                    kind: "other",
                    family: 16,
                    protocol: 0,
                    socket_type: null,
                  },
                  state: null,
                })
              : null,
      });
      h.snapshot({
        ...initial,
        fd_relations: kind === "port" || kind === "file" ? [] : [relation],
      });
      const activity: Partial<SpaceActivity> =
        kind === "file"
          ? {
              files: [
                {
                  process_id: source,
                  file: {
                    device: { major: 8, minor: 1 },
                    inode: "5",
                    generation: 0,
                  },
                  path: "/tmp/data",
                  write: true,
                  bytes: 100,
                  count: 64,
                },
              ],
            }
          : {
              ipc: [
                {
                  process_id: source,
                  resource: endpoint.resource,
                  write: true,
                  bytes: 100,
                  count: 64,
                },
              ],
            };
      h.activity(activity);
      const retained = h.renderer.activeResources(1000);
      assert.ok(retained.processes.has(key(source)));
      if (kind === "pipe") assert.ok(retained.processes.has(key(target)));
      if (kind === "network") assert.equal(retained.networks.size, 1);
      h.snapshot({ processes: [], fd_relations: [] }, 1100);
      h.activity({}, 1200);
      assert.deepEqual(
        h.activity(activity, 1200).routes,
        [],
        "retained observations do not admit late events",
      );
      if (kind === "file") {
        assert.equal(h.data.recentFiles.entries.size, 1);
        assert.equal(
          [...h.data.recentFiles.entries.values()][0].writeBytes,
          100,
        );
        assert.equal(h.data.pruneFiles(1300), false);
      }
      if (kind === "netlink") {
        assert.equal(h.data.netlinkSockets.entries.size, 1);
        assert.equal(
          [...h.data.netlinkSockets.entries.values()][0].writeBytes,
          100,
        );
      }
      if (kind === "pipe" || kind === "network") {
        assert.equal(h.data.displaySnapshot.fd_relations.length, 1);
        assert.equal(h.data.edgeStats.get(kind)!.bytes, 100);
      }
      h.frame(kind === "port" ? 2999 : 3499);
      assert.ok(h.data.nodes.has(key(source)));
      h.frame(kind === "port" ? 3000 : 3500);
      assert.equal(h.data.nodes.size, 0);
      assert.equal(h.data.displaySnapshot.fd_relations.length, 0);
      assert.equal(h.data.network.size, 0);
      assert.equal(h.data.recentFiles.entries.size, 0);
      assert.equal(h.data.netlinkSockets.entries.size, 0);
    } finally {
      h.dispose();
    }
  },
);

test("overlapping animations, PID reuse, caps and clearing do not leave retired processes", () => {
  const h = setup();
  try {
    h.snapshot(initial);
    h.activity({ signals: [signal] });
    h.activity({ signals: [signal] }, 2000);
    const reused = processInfo({
      identity: { ...target, start_time_ticks: 99 },
      name: "reused",
    });
    h.snapshot({ processes: [reused], fd_relations: [] }, 2100);
    assert.equal(h.data.nodes.size, 3);
    h.frame(3100);
    assert.equal(
      h.data.nodes.size,
      3,
      "the second animation outlives the first",
    );
    assert.equal(h.data.nodes.get(key(target))!.name, "target");
    h.frame(4100);
    assert.deepEqual([...h.data.nodes.keys()], [key(reused.identity)]);
    h.snapshot(initial, 5000);
    h.activity({ signals: Array(5000).fill(signal) }, 5000);
    assert.equal(h.renderer.signalVisuals().length, 256);
    h.snapshot({ processes: [], fd_relations: [] }, 5100);
    h.clear(5200);
    assert.equal(h.data.nodes.size, 0);
    assert.equal(h.renderer.activeResources(5200).processes.size, 0);
    h.snapshot(initial, 6000);
    h.activity({ signals: [signal] }, 6000);
    h.snapshot({ processes: [], fd_relations: [] }, 6100);
    h.snapshot({ processes: [], fd_relations: [] }, 6200, true);
    assert.equal(h.data.nodes.size, 0, "rearranging clears retained display");
  } finally {
    h.dispose();
  }
});

test("animation caps release identities referenced only by evicted flights or particles", () => {
  const h = setup();
  try {
    h.snapshot(initial);
    h.activity({ signals: [signal] });
    const idle = initial.processes[2].identity;
    h.activity({
      signals: Array(256).fill({
        ...signal,
        source_id: idle,
        destination_id: idle,
        src_pid: idle.pid,
        dst_pid: idle.pid,
      }),
    });
    h.snapshot({ processes: [initial.processes[2]], fd_relations: [] }, 1100);
    assert.deepEqual([...h.data.nodes.keys()], [key(idle)]);
    h.clear(1200);
    h.snapshot(initial, 2000);
    h.activity(
      {
        ipc: [
          {
            process_id: source,
            resource: fdEndpoint().resource,
            write: true,
            bytes: 1,
            count: 1,
          },
        ],
      },
      2000,
    );
    h.activity(
      {
        ipc: Array(8192).fill({
          process_id: idle,
          resource: fdEndpoint().resource,
          write: true,
          bytes: 1,
          count: 1,
        }),
      },
      2000,
    );
    h.snapshot({ processes: [], fd_relations: [] }, 2100);
    assert.deepEqual([...h.data.nodes.keys()], [key(idle)]);
    h.frame(4000);
    assert.equal(h.data.nodes.size, 0);
  } finally {
    h.dispose();
  }
});

test("closed live connections are retained for rendering but cannot route new activity", () => {
  const h = setup();
  try {
    const relation = fdRelation({
      endpoint: fdEndpoint({ process_id: source }),
      peer: fdEndpoint({ process_id: target }),
    });
    h.snapshot({ ...initial, fd_relations: [relation] });
    const activity = {
      ipc: [
        {
          process_id: source,
          resource: relation.endpoint.resource,
          write: true,
          bytes: 100,
          count: 1,
        },
      ],
    };
    h.activity(activity);
    h.snapshot(initial, 1100);
    assert.equal(h.data.displaySnapshot.fd_relations.length, 1);
    assert.deepEqual(
      h.activity(activity, 1200).routes.map((r) => r.kind),
      ["port"],
    );
    h.frame(3100);
    assert.equal(h.data.displaySnapshot.fd_relations.length, 0);
    assert.equal(
      h.data.nodes.size,
      3,
      "live processes remain after a retained route expires",
    );
  } finally {
    h.dispose();
  }
});

test("positive socket exclusion cancels retention and reduced-motion data expires promptly", () => {
  const h = setup(true);
  try {
    const relation = fdRelation({
      endpoint: fdEndpoint({
        process_id: source,
        kind: "socket",
        resource: {
          kind: "socket",
          device: { major: 0, minor: 1 },
          inode: "10",
        },
      }),
      socket: socketEndpoint({
        network_peer: true,
        remote: { ip: "203.0.113.1", port: 443 },
      }),
    });
    const snapshot = { ...initial, fd_relations: [relation] };
    const activity = {
      ipc: [
        {
          process_id: source,
          resource: relation.endpoint.resource,
          write: true,
          bytes: 100,
          count: 64,
        },
      ],
    };
    h.snapshot(snapshot);
    h.activity(activity);
    h.snapshot({ processes: [], fd_relations: [] }, 1100);
    h.frame(1149);
    assert.equal(h.data.nodes.size, 1);
    h.frame(1150);
    assert.equal(h.data.nodes.size, 0);
    h.snapshot(snapshot, 2000);
    h.activity(activity, 2000);
    h.snapshot(
      {
        processes: [],
        fd_relations: [
          {
            ...relation,
            endpoint: { ...relation.endpoint, connection_status: "listening" },
            socket: socketEndpoint({ state: { kind: "listen" } }),
          },
        ],
      },
      2050,
    );
    assert.ok(
      h.data.suppressedSocketOwners.has(socketOwnerKey(relation.endpoint)),
    );
    assert.equal(h.renderer.activeResources(2050).processes.size, 0);
    assert.equal(h.data.nodes.size, 0);
    assert.equal(h.data.network.size, 0);
  } finally {
    h.dispose();
  }
});

test("paused replay freezes CPU glow and signal motion independently of wall frames", () => {
  let replayTime = 1000;
  setSpaceClock(() => replayTime);
  const h = setup();
  try {
    h.snapshot(initial);
    h.activity({
      signals: [signal],
      cpu: [
        {
          process_id: source,
          runtime_ns: 100_000_000,
          switches: 1,
          running_threads: 1,
          cpus: [0],
        },
      ],
    });
    h.frame(1000);
    const glow = h.scene.cpuGlowVisual(key(source));
    h.frame(100_000);
    assert.deepEqual(h.scene.cpuGlowVisual(key(source)), glow);
    assert.equal(h.renderer.signalVisuals()[0].arrived, false);
    replayTime = 2750;
    h.frame(100_010);
    assert.equal(h.renderer.signalVisuals()[0].arrived, true);
    assert.notDeepEqual(h.scene.cpuGlowVisual(key(source)), glow);
  } finally {
    h.dispose();
    setSpaceClock();
  }
});
