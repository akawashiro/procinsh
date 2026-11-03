// @vitest-environment jsdom

import { describe, expect, test } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import * as T from "three";
import { layerPivot } from "../src/space/camera.js";
import {
  ALL_HOSTS,
  HOST_LAYER_GAP,
  addressHost,
  composeHosts,
  hostLayers,
  qualifyActivity,
  qualifySnapshot,
} from "../src/space/hosts.js";
import { SpaceDataStore } from "../src/space/data.js";
import { createSpaceScene } from "../src/space/scene.js";
import { createSpaceDetails } from "../src/space/details.js";
import { spaceElement } from "../src/space/dom-types.js";
import { key } from "../src/space/model.js";
import {
  fdEndpoint,
  fdRelation,
  processInfo,
  socketEndpoint,
  spaceActivity,
} from "./support/fixtures.js";
import { TestEventSource } from "./support/mocks.js";
import type {
  CrossHostLinks,
  HardwareDevice,
  HostsResponse,
  HostSummary,
  SystemSnapshot,
} from "../src/shared/api-types.js";

const pa = { pid: 10, start_time_ticks: 1 },
  pb = { pid: 10, start_time_ticks: 1 }; // the same identity on another host
const socket = (id: string, process = pa, inode = "7") =>
  fdRelation({
    id,
    endpoint: fdEndpoint({
      process_id: process,
      kind: "socket",
      resource: { kind: "socket", device: { major: 0, minor: 8 }, inode },
    }),
    socket: socketEndpoint({
      network_peer: true,
      local: { ip: "10.0.0.1", port: 5000 },
      remote: { ip: "10.0.0.2", port: 443 },
    }),
    label: "TCP ESTABLISHED 10.0.0.2:443",
  });
const gpu = (address: string): HardwareDevice => ({
  address,
  parent: null,
  role: "endpoint",
  kind: "gpu",
  label: "GPU",
  class: null,
  vendor_id: null,
  device_id: null,
  driver: null,
  numa_node: null,
  link: null,
  power_state: null,
  runtime_status: null,
  iommu_group: null,
  bars: [],
  gpu: null,
  rdma: [],
  netdevs: [],
});
const host = (name: string, local = false): HostSummary => ({
  name,
  local,
  status: { state: "online", since: 0 },
  machine_id: name,
  version: "0",
  kernel: null,
  arch: "x86_64",
});
// Both machines have a GPU at the same PCI address, used by the same identity.
const hardware = (process: typeof pa) => ({
  hardware_devices: [gpu("0000:01:00.0")],
  hardware_paths: [],
  device_handles: [
    {
      id: "10:1:nvidia:0000:01:00.0",
      process_id: process,
      device: "0000:01:00.0",
      kind: "nvidia" as const,
      fd: 3,
      fd_count: 1,
    },
  ],
});
const snapshotA: SystemSnapshot = {
  processes: [processInfo({ identity: pa, name: "client" })],
  fd_relations: [socket("tcp")],
  ...hardware(pa),
};
const snapshotB: SystemSnapshot = {
  processes: [processInfo({ identity: pb, parent_id: null, name: "server" })],
  fd_relations: [socket("tcp", pb, "9")],
  ...hardware(pb),
};
const links: CrossHostLinks = {
  sockets: [
    { a: { host: "a", relation: "tcp" }, b: { host: "b", relation: "tcp" } },
  ],
};

describe("multi-host composition", () => {
  test("qualification tags every identity and keeps hosts apart", () => {
    const a = qualifySnapshot("a", snapshotA);
    expect(a.processes[0].identity).toEqual({ ...pa, host: "a" });
    expect(a.fd_relations[0].id).toBe("a@tcp");
    expect(a.fd_relations[0].endpoint.process_id.host).toBe("a");
    expect(a.hardware_devices![0].address).toBe("a@0000:01:00.0");
    expect(a.device_handles![0]).toMatchObject({
      device: "a@0000:01:00.0",
      process_id: { host: "a" },
    });
    expect(addressHost(a.hardware_devices![0].address)).toBe("a");
    expect(addressHost("0000:01:00.0")).toBeUndefined();
    expect(key(a.processes[0].identity)).not.toBe(
      key(qualifySnapshot("b", snapshotB).processes[0].identity),
    );
    const activity = qualifyActivity(
      "a",
      spaceActivity({
        ipc: [
          {
            process_id: pa,
            resource: snapshotA.fd_relations[0].endpoint.resource,
            write: true,
            bytes: 1,
            count: 1,
          },
        ],
        signals: [
          {
            timestamp_ns: 0,
            src_pid: 10,
            dst_pid: 10,
            signal: 15,
            source_id: pa,
            destination_id: pa,
          },
        ],
      }),
    );
    expect(activity.ipc![0].process_id.host).toBe("a");
    expect(activity.signals![0].destination_id.host).toBe("a");
  });

  test("cross-host links replace both external ends", () => {
    const composed = composeHosts(
      new Map([
        ["a", qualifySnapshot("a", snapshotA)],
        ["b", qualifySnapshot("b", snapshotB)],
      ]),
      links,
    );
    expect(composed.processes).toHaveLength(2);
    expect(composed.hardware_devices!.map((d) => d.address)).toEqual([
      "a@0000:01:00.0",
      "b@0000:01:00.0",
    ]);
    expect(composed.fd_relations.map((r) => r.id)).toEqual(["a@tcp|b@tcp"]);
    const [tcp] = composed.fd_relations;
    expect(tcp.peer?.process_id.host).toBe("b");
    expect(tcp.label).toBe("TCP ESTABLISHED 10.0.0.2:443 ↔ b");
    // Links to relations not received yet keep the external ends.
    const partial = composeHosts(
      new Map([["a", qualifySnapshot("a", snapshotA)]]),
      links,
    );
    expect(partial.fd_relations.map((r) => r.id)).toEqual(["a@tcp"]);
    expect([...hostLayers(["a", "b"], partial)]).toEqual([
      ["a", 0],
      ["b", -HOST_LAYER_GAP],
    ]);
    expect([...hostLayers([], composed).keys()]).toEqual(["a", "b"]);
  });

  test("the merged stream stacks hosts and routes their activity", () => {
    let source: TestEventSource | undefined;
    const data = new SpaceDataStore(undefined, undefined, (url) => {
      source = new TestEventSource(url);
      return source.asEventSource();
    });
    data.host = ALL_HOSTS;
    data.start();
    expect(source!.url).toBe("/api/system/events?host=all");
    const hosts: HostsResponse = {
      local: "a",
      hosts: [host("a", true), host("b")],
    };
    source!.emit("hosts", hosts);
    source!.emit("snapshot", {
      ...snapshotA,
      host: "a",
      kind: "full",
      sequence: 1,
    });
    source!.emit("snapshot", {
      ...snapshotB,
      host: "b",
      kind: "full",
      sequence: 4,
    });
    source!.emit("links", links);
    const a = data.nodes.get(key({ ...pa, host: "a" }))!,
      b = data.nodes.get(key({ ...pb, host: "b" }))!;
    expect([a.pos.z, b.pos.z]).toEqual([0, -HOST_LAYER_GAP]);
    expect(data.hostLayerZ.get("b")).toBe(-HOST_LAYER_GAP);
    expect(data.snapshot.fd_relations.map((r) => r.id)).toEqual([
      "a@tcp|b@tcp",
    ]);
    expect(data.network.size).toBe(0);
    // Each host's hardware bay sits on its own layer.
    expect(data.hardwarePositions.get("a@0000:01:00.0")?.z).toBe(0);
    expect(data.hardwarePositions.get("b@0000:01:00.0")?.z).toBe(
      -HOST_LAYER_GAP,
    );
    // Deltas apply to each host's own baseline and sequence.
    source!.emit("snapshot", {
      kind: "delta",
      host: "b",
      sequence: 5,
      base_sequence: 4,
      processes: [
        ...snapshotB.processes,
        processInfo({
          identity: { pid: 11, start_time_ticks: 2 },
          name: "worker",
        }),
      ],
      fd_relations_delta: { upsert: [], remove: [] },
    });
    expect(data.nodes.get("b@11:2")?.pos.z).toBe(-HOST_LAYER_GAP);
    expect(data.nodes.has("a@10:1")).toBe(true);
    source!.emit("activity", {
      ...spaceActivity({
        ipc: [
          {
            process_id: pb,
            resource: snapshotB.fd_relations[0].endpoint.resource,
            write: true,
            bytes: 8,
            count: 1,
          },
        ],
      }),
      host: "b",
    });
    expect(data.edgeStats.get("a@tcp|b@tcp")?.bytes).toBe(8);
    // An offline host's processes leave the merged view.
    source!.emit("hosts", {
      local: "a",
      hosts: [
        host("a", true),
        { ...host("b"), status: { state: "offline", since: 1 } },
      ],
    });
    expect(data.hosts[1].status.state).toBe("offline");
    expect(data.nodes.has("b@10:1")).toBe(false);
    expect(data.nodes.has("a@10:1")).toBe(true);
    data.stop();
  });

  test("a single remote host streams by name and reports its loss", () => {
    let source: TestEventSource | undefined;
    const statuses: (string | null)[] = [];
    const data = new SpaceDataStore(
      {
        snapshot() {},
        activity() {},
        reset() {},
        gap() {},
        status: (message) => statuses.push(message),
      },
      undefined,
      (url) => {
        source = new TestEventSource(url);
        return source.asEventSource();
      },
    );
    data.host = "node b";
    data.start();
    expect(source!.url).toBe("/api/system/events?host=node+b");
    source!.emit("snapshot", { ...snapshotB, kind: "full", sequence: 1 });
    expect(data.nodes.has("10:1")).toBe(true);
    source!.error();
    expect(statuses.at(-1)).toBe("Host node b is unavailable. Retrying…");
    data.stop();
  });

  test("scene floors each host and joins layers", () => {
    const data = new SpaceDataStore();
    data.hosts = [host("a", true), host("b")];
    data.replaceSnapshot(
      composeHosts(
        new Map([
          ["a", qualifySnapshot("a", snapshotA)],
          ["b", qualifySnapshot("b", snapshotB)],
        ]),
        links,
      ),
      true,
    );
    const scene = createSpaceScene();
    const snapshot = data.displaySnapshot;
    scene.rebuild(
      {
        snapshot,
        nodes: data.nodes,
        network: data.network,
        networkPositions: data.networkPositions,
        files: new Map(),
        netlink: new Map(),
        filePositions: new Map(),
        netlinkPositions: new Map(),
        visible: new Set(data.nodes.keys()),
        hardware: {
          devices: snapshot.hardware_devices ?? [],
          paths: snapshot.hardware_paths ?? [],
          handles: snapshot.device_handles ?? [],
          positions: data.hardwarePositions,
        },
        hostLabels: new Map([["b", "b · offline"]]),
      },
      null,
    );
    const view = scene.renderView();
    expect(view.layerViews!.map((l) => [l.host, l.label])).toEqual([
      ["a", "a"],
      ["b", "b · offline"],
    ]);
    expect(view.layerViews![1].pos.z).toBeLessThan(-HOST_LAYER_GAP);
    const tcp = view.edgeViews.find((e) => e.e.id === "a@tcp|b@tcp")!;
    expect(tcp.curve.getPoint(0).z).toBeGreaterThan(0);
    expect(tcp.curve.getPoint(1).z).toBeLessThan(-HOST_LAYER_GAP + 8.2);
    expect(
      view.hardwareViews!.map((v) => [v.device.address, v.pos.z < 0]),
    ).toEqual([
      ["a@0000:01:00.0", false],
      ["b@0000:01:00.0", true],
    ]);
    expect(
      scene.pickingView().objects.some((o) => o.name === "host-layers"),
    ).toBe(false);
    expect(scene.root.getObjectByName("host-layers")).toBeInstanceOf(
      T.InstancedMesh,
    );
    scene.dispose();
  });
});

describe("process pages of remote hosts", () => {
  test("details link merged and single remote host processes with their host", () => {
    document.body.innerHTML = readFileSync(
      resolve(import.meta.dirname, "../space/index.html"),
      "utf8",
    );
    const details = createSpaceDetails(
      { connection() {}, network() {} },
      spaceElement,
    );
    const update = (
      snapshot: SystemSnapshot,
      host: string | null,
      process: string,
    ) => {
      const data = new SpaceDataStore();
      data.replaceSnapshot(snapshot);
      details.update(
        {
          host,
          nodes: data.nodes,
          snapshot: data.displaySnapshot,
          network: data.network,
          edgeStats: new Map(),
          files: new Map(),
          netlink: new Map(),
        },
        { process, connection: null, network: null, file: null, netlink: null },
      );
    };
    update(qualifySnapshot("b", snapshotB), null, "b@10:1");
    expect(spaceElement("name").getAttribute("href")).toBe(
      "/process/10?start_time_ticks=1&host=b",
    );
    expect(spaceElement("pid").textContent).toContain("b · PID 10");
    update(snapshotB, "b", "10:1");
    expect(spaceElement("name").getAttribute("href")).toBe(
      "/process/10?start_time_ticks=1&host=b",
    );
    update(snapshotA, null, "10:1");
    expect(spaceElement("name").getAttribute("href")).toBe(
      "/process/10?start_time_ticks=1",
    );
  });
});

test("rotations pivot on the layer in view without moving the picture", () => {
  const layers = [
    { z: 4, min: { x: -10, y: -10 }, max: { x: 10, y: 10 } },
    { z: -52, min: { x: 40, y: -10 }, max: { x: 60, y: 10 } },
  ];
  const camera = new T.Vector3(0, -100, 100);
  // Looking down at the second layer's floor, which the first does not cover.
  const pivot = layerPivot(camera, new T.Vector3(50, 0, -52), layers)!;
  expect(pivot.z).toBe(-52);
  expect(Math.abs(pivot.x - 50) < 1e-9 && Math.abs(pivot.y) < 1e-9).toBe(true);
  // The first layer in front wins when the ray crosses both floors.
  const front = layerPivot(camera, new T.Vector3(0, 0, 4), [
    ...layers,
    { z: -52, min: { x: -10, y: -10 }, max: { x: 10, y: 60 } },
  ])!;
  expect(front.z).toBe(4);
  // The pivot stays on the view ray, so the picture does not jump.
  const direction = new T.Vector3(50, 0, -52).sub(camera).normalize();
  expect(
    pivot.clone().sub(camera).normalize().distanceTo(direction) < 1e-9,
  ).toBe(true);
  // A ray missing every floor picks the nearest floor; a level view has none.
  expect(layerPivot(camera, new T.Vector3(70, 0, -52), layers)!.z).toBe(-52);
  expect(layerPivot(camera, new T.Vector3(0, 0, 100), layers)).toBe(undefined);
});
