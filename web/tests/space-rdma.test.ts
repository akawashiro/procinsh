// @vitest-environment jsdom

import { describe, expect, test } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import * as T from "three";
import {
  isRdmaRelationVisible,
  rdmaDeviceId,
  rdmaDeviceLayout,
  rdmaModel,
} from "../src/space/rdma.js";
import { SpaceDataStore } from "../src/space/data.js";
import {
  ALL_HOSTS,
  HOST_LAYER_GAP,
  composeHosts,
  qualifySnapshot,
} from "../src/space/hosts.js";
import { createSpaceScene } from "../src/space/scene.js";
import { createSpaceDetails } from "../src/space/details.js";
import { spaceElement } from "../src/space/dom-types.js";
import { key } from "../src/space/model.js";
import {
  fdEndpoint,
  fdRelation,
  processInfo,
  socketEndpoint,
  rdmaEndpoint,
  rdmaPort,
  rdmaRelation,
} from "./support/fixtures.js";
import { TestEventSource } from "./support/mocks.js";
import type {
  CrossHostLinks,
  HardwareDevice,
  HostsResponse,
  RdmaRelation,
  SystemSnapshot,
} from "../src/shared/api-types.js";
import type { SelectionState } from "../src/space/types.js";

const process = { pid: 42, start_time_ticks: 7 };
const REMOTE_GID = "fe80:0000:0000:0000:f452:1403:0080:de51";
const nfs = rdmaRelation({
  cm: {
    id: 0,
    owner: { kind: "kernel", name: "rpcrdma" },
    state: { kind: "connect" },
    port_space: { kind: "tcp" },
    source: { kind: "inet", ip: "10.10.10.2", port: 58985 },
    destination: { kind: "inet", ip: "10.10.10.1", port: 20049 },
  },
  remote_gid: REMOTE_GID,
  label: "RDMA RC QP 524 → QP 650 10.10.10.1:20049",
});
const management = rdmaRelation({
  endpoint: rdmaEndpoint({
    qpn: 1,
    qp_type: { kind: "gsi" },
    owner: { kind: "kernel", name: "ib_core" },
  }),
  remote_qpn: null,
});
const user = (qpn: number, extra: Partial<RdmaRelation> = {}) =>
  rdmaRelation({
    endpoint: rdmaEndpoint({
      qpn,
      owner: { kind: "process", process_id: process },
    }),
    remote_qpn: qpn + 100,
    remote_gid: REMOTE_GID,
    ...extra,
  });
const loopback = rdmaRelation({
  id: "rdma:ib0/1:10/rdma:ib0/1:20",
  endpoint: rdmaEndpoint({
    qpn: 10,
    owner: { kind: "process", process_id: process },
  }),
  peer: rdmaEndpoint({ qpn: 20, owner: { kind: "unknown", pid: 9 } }),
  remote_qpn: 20,
  label: "RDMA RC QP 10 ↔ QP 20",
});
const sceneRdma = (data: SpaceDataStore) => ({
  groups: data.rdma,
  links: data.rdmaLinks,
  devices: data.rdmaDevices,
  positions: data.rdmaPositions,
  devicePositions: data.rdmaDevicePositions,
});
const snapshot = (rdma: RdmaRelation[]): SystemSnapshot => ({
  processes: [processInfo({ identity: process, name: "mpi" })],
  fd_relations: [],
  rdma_relations: rdma,
  rdma_ports: [rdmaPort(), rdmaPort({ port: 2, state: "DOWN", lid: null })],
});

describe("RDMA SPACE model", () => {
  test("groups unpaired QPs by anchor and remote, links local pairs", () => {
    const live = new Set([key(process)]);
    const { groups, links, devices } = rdmaModel(
      [nfs, management, user(30), user(31), loopback],
      [rdmaPort(), rdmaPort({ device: "mlx5_0" })],
      live,
    );
    expect(isRdmaRelationVisible(management)).toBe(false);
    expect([...devices.values()].map((d) => d.device)).toEqual([
      "ib0",
      "mlx5_0",
    ]);
    expect(devices.get(rdmaDeviceId("ib0"))!.ports.map((p) => p.port)).toEqual([
      1,
    ]);
    const labels = [...groups.values()].map((g) => [g.anchor, g.label]);
    expect(labels).toEqual([
      [rdmaDeviceId("ib0"), "rpcrdma · RDMA RC 10.10.10.1:20049 ×1"],
      [key(process), `RDMA RC ${REMOTE_GID} ×2`],
    ]);
    expect(links).toEqual([
      {
        id: loopback.id,
        relation: loopback,
        from: key(process),
        to: rdmaDeviceId("ib0"),
      },
    ]);
    // A process outside the snapshot anchors its QPs to the device.
    const orphan = rdmaModel([user(30)], [], new Set());
    expect([...orphan.groups.values()][0].anchor).toBe(rdmaDeviceId("ib0"));
    expect([...orphan.devices.keys()]).toEqual([rdmaDeviceId("ib0")]);
    // Hosts of the merged view keep same-named devices apart.
    const merged = rdmaModel(
      [],
      [rdmaPort({ host: "a" }), rdmaPort({ host: "b" })],
      new Set(),
    );
    expect([...merged.devices.keys()]).toEqual([
      rdmaDeviceId("ib0", "a"),
      rdmaDeviceId("ib0", "b"),
    ]);
  });

  test("device anchors sit right of processes and keep their positions", () => {
    const first = rdmaDeviceLayout(
      [rdmaDeviceId("ib0"), rdmaDeviceId("mlx5_0")],
      [
        { x: -3, y: -6 },
        { x: 10, y: 4 },
      ],
    );
    expect(first.get(rdmaDeviceId("ib0"))).toEqual({ x: 18, y: -6, z: 0 });
    expect(first.get(rdmaDeviceId("mlx5_0"))).toEqual({ x: 18, y: 0, z: 0 });
    const moved = rdmaDeviceLayout(
      ["ib0", "mlx5_0", "rxe0"].map((d) => rdmaDeviceId(d)),
      [{ x: 30, y: 0 }],
      first,
    );
    expect(moved.get(rdmaDeviceId("ib0"))).toEqual(
      first.get(rdmaDeviceId("ib0")),
    );
    expect(moved.get(rdmaDeviceId("rxe0"))).toEqual({ x: 39, y: 0, z: 0 });
    expect(rdmaDeviceLayout([], []).size).toBe(0);
  });

  test("SSE deltas update RDMA relations and ports; markers avoid network cells", () => {
    let source: TestEventSource | undefined;
    const data = new SpaceDataStore(undefined, undefined, (url) => {
      source = new TestEventSource(url);
      return source.asEventSource();
    });
    data.start();
    source!.emit("snapshot", {
      ...snapshot([nfs, user(30)]),
      kind: "full",
      sequence: 1,
    });
    expect(data.rdma.size).toBe(2);
    expect(data.rdmaDevices.has(rdmaDeviceId("ib0"))).toBe(true);
    source!.emit("snapshot", {
      kind: "delta",
      sequence: 2,
      base_sequence: 1,
      processes: snapshot([]).processes,
      fd_relations: [],
      rdma_ports: [rdmaPort()],
      rdma_relations_delta: { upsert: [user(31)], remove: [nfs.id] },
    });
    expect(data.snapshot.rdma_relations!.map((r) => r.id)).toEqual([
      user(30).id,
      user(31).id,
    ]);
    expect(data.snapshot.rdma_ports).toHaveLength(1);
    expect("rdma_relations_delta" in data.snapshot).toBe(false);
    const [group] = data.rdma.values();
    expect(group.members).toHaveLength(2);
    // Network and RDMA markers above one process share a lattice without overlap.
    data.replaceSnapshot({
      ...data.snapshot,
      fd_relations: [
        fdRelation({
          id: "tcp",
          endpoint: fdEndpoint({
            process_id: process,
            kind: "socket",
            resource: {
              kind: "socket",
              device: { major: 0, minor: 8 },
              inode: "9",
            },
          }),
          socket: socketEndpoint({
            network_peer: true,
            remote: { ip: "192.0.2.1", port: 443 },
          }),
        }),
      ],
    });
    const cells = [
      ...data.networkPositions.values(),
      ...data.rdmaPositions.values(),
    ].map((p) => `${p.x}:${p.y}:${p.z}`);
    expect(data.networkPositions.size).toBe(1);
    expect(new Set(cells).size).toBe(2);
    // An older server without RDMA fields keeps an empty RDMA view.
    source!.emit("snapshot", {
      kind: "full",
      sequence: 3,
      processes: snapshot([]).processes,
      fd_relations: [],
    });
    expect(data.rdma.size).toBe(0);
    expect(data.snapshot.rdma_relations).toEqual([]);
    data.stop();
  });

  test("scene draws devices, markers and links that pick to RDMA IDs", () => {
    const data = new SpaceDataStore();
    data.replaceSnapshot(snapshot([nfs, user(30), loopback, management]));
    const scene = createSpaceScene();
    scene.rebuild(
      {
        snapshot: data.displaySnapshot,
        nodes: data.nodes,
        network: data.network,
        networkPositions: data.networkPositions,
        files: new Map(),
        netlink: new Map(),
        filePositions: new Map(),
        netlinkPositions: new Map(),
        visible: new Set(data.nodes.keys()),
        rdma: sceneRdma(data),
      },
      null,
    );
    const view = scene.renderView();
    expect(view.rdmaDeviceViews!.map((v) => v.device.label)).toEqual(["ib0"]);
    expect(view.rdmaViews!.map((v) => v.group.label).sort()).toEqual([
      `RDMA RC ${REMOTE_GID} ×1`,
      "rpcrdma · RDMA RC 10.10.10.1:20049 ×1",
    ]);
    expect(view.rdmaLinkViews!.map((v) => v.link.id)).toEqual([loopback.id]);
    const markers = scene.root.getObjectByName("rdma-markers")!;
    expect(markers.userData.rdma).toEqual(
      view.rdmaViews!.map((v) => v.group.id),
    );
    const lines = scene.root.getObjectByName("rdma-lines") as T.LineSegments;
    expect(new Set(lines.userData.rdmaPaths)).toEqual(
      new Set([...view.rdmaViews!.map((v) => v.group.id), loopback.id]),
    );
    // Markers start above their anchors: the process roof or the device plate.
    for (const v of view.rdmaViews!) {
      const start = v.curve.getPoint(0);
      expect(start.z).toBeCloseTo(v.group.device ? 0.5 : 8);
    }
    const selection: SelectionState = {
      process: null,
      connection: null,
      network: null,
      file: null,
      netlink: null,
      rdma: loopback.id,
    };
    scene.updateSelection(selection);
    expect(scene.root.getObjectByName("selected-connection")!.visible).toBe(
      true,
    );
    // Process search hides process-anchored QPs, while device anchors remain.
    scene.rebuild(
      {
        snapshot: data.displaySnapshot,
        nodes: data.nodes,
        network: data.network,
        networkPositions: data.networkPositions,
        files: new Map(),
        netlink: new Map(),
        filePositions: new Map(),
        netlinkPositions: new Map(),
        visible: new Set(),
        rdma: sceneRdma(data),
      },
      null,
    );
    expect(scene.renderView().rdmaViews!.map((v) => v.group.device)).toEqual([
      "ib0",
    ]);
    expect(scene.renderView().rdmaLinkViews).toEqual([]);
    scene.dispose();
  });

  test("details show QP addresses, local ports and device ports", () => {
    document.body.innerHTML = readFileSync(
      resolve(import.meta.dirname, "../space/index.html"),
      "utf8",
    );
    const data = new SpaceDataStore();
    data.replaceSnapshot(snapshot([nfs, loopback]));
    const details = createSpaceDetails(
      { connection() {}, network() {} },
      spaceElement,
    );
    const input = {
      nodes: data.nodes,
      snapshot: data.displaySnapshot,
      network: data.network,
      edgeStats: new Map(),
      files: new Map(),
      netlink: new Map(),
      rdma: {
        groups: data.rdma,
        links: data.rdmaLinks,
        devices: data.rdmaDevices,
      },
    };
    const select = (rdma: string): SelectionState => ({
      process: null,
      connection: null,
      network: null,
      file: null,
      netlink: null,
      rdma,
    });
    const [group] = data.rdma.values();
    details.update(input, select(group.id));
    const text = spaceElement("connection-endpoints").textContent;
    expect(spaceElement("connection-label").textContent).toBe(group.label);
    expect(spaceElement("connection-state").textContent).toBe(
      "RDMA queue pairs to a remote destination",
    );
    expect(spaceElement("connection-facts").textContent).toContain(
      "bypass the kernel",
    );
    expect(text).toContain("Kernel · rdma_cm");
    expect(text).toContain("QP · ib0/1 QP 524 · RC · RTS");
    expect(text).toContain(
      "Local LID 2 · GID fe80:0000:0000:0000:0002:c903:0028:45c7",
    );
    expect(text).toContain(`Remote QP 650 · GID ${REMOTE_GID}`);
    expect(text).toContain(
      "CM CONNECT · TCP · rpcrdma · 10.10.10.2:58985 → 10.10.10.1:20049",
    );
    details.update(input, select(loopback.id));
    expect(spaceElement("connection-state").textContent).toBe(
      "Connected queue pairs on this host",
    );
    expect(spaceElement("connection-endpoints").textContent).toContain(
      "PEER · ib0/1 QP 20",
    );
    expect(spaceElement("connection-endpoints").textContent).toContain("mpi");
    details.update(input, select(rdmaDeviceId("ib0")));
    expect(spaceElement("connection-state").textContent).toBe("RDMA device");
    expect(spaceElement("connection-endpoints").textContent).toContain(
      "Port 2 · DOWN",
    );
    expect(spaceElement("connection-endpoints").textContent).toContain(
      "InfiniBand · 40 Gb/sec (4X QDR)",
    );
    // Unknown IDs leave the panel untouched, like other stale selections.
    details.update({ ...input, rdma: undefined }, select("missing"));
  });

  test("a device plate rests on its NIC in the hardware bay", () => {
    const nic = (address: string, name: string): HardwareDevice => ({
      address,
      parent: null,
      role: "endpoint",
      kind: "rdma",
      label: name,
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
      rdma: [{ name, hca_type: null, fw_ver: null }],
      netdevs: [],
    });
    const data = new SpaceDataStore();
    data.replaceSnapshot({
      ...snapshot([nfs]),
      rdma_ports: [rdmaPort(), rdmaPort({ device: "rxe0" })],
      hardware_devices: [nic("0000:85:00.0", "ib0")],
    });
    const bay = data.hardwarePositions.get("0000:85:00.0")!;
    expect(data.rdmaDevicePositions.get(rdmaDeviceId("ib0"))).toEqual({
      ...bay,
      z: bay.z + 0.5,
    });
    // A device without a PCI NIC, such as Soft-RoCE, keeps the column right of the processes.
    const soft = data.rdmaDevicePositions.get(rdmaDeviceId("rxe0"))!;
    expect(soft.z).toBe(0);
    expect(soft.x).toBeGreaterThan(
      Math.max(...[...data.nodes.values()].map((n) => n.pos.x)),
    );
    // Kernel QP markers rise above the NIC.
    const [group] = [...data.rdma.values()];
    expect(group.anchor).toBe(rdmaDeviceId("ib0"));
    expect(data.rdmaPositions.get(group.id)!.z).toBeGreaterThan(bay.z);
  });
});

describe("RDMA across hosts", () => {
  const pa = { pid: 10, start_time_ticks: 1 },
    pb = { pid: 10, start_time_ticks: 1 }; // the same identity on another host
  const qp = (qpn: number, remote: number, owner = pa) =>
    rdmaRelation({
      endpoint: rdmaEndpoint({
        qpn,
        owner: { kind: "process", process_id: owner },
      }),
      cm: {
        id: 1,
        owner: { kind: "kernel", name: "rpcrdma" },
        state: { kind: "connect" },
        port_space: { kind: "tcp" },
        source: null,
        destination: null,
      },
      remote_qpn: remote,
    });
  const hostSnapshot = (
    name: string,
    rdma: RdmaRelation[],
  ): SystemSnapshot => ({
    processes: [processInfo({ identity: pa, name })],
    fd_relations: [],
    rdma_relations: rdma,
    rdma_ports: [rdmaPort()],
  });
  const links: CrossHostLinks = {
    sockets: [],
    rdma: [
      {
        a: { host: "a", relation: "rdma:ib0/1:10" },
        b: { host: "b", relation: "rdma:ib0/1:20" },
        evidence: { gid: true, cm: false },
      },
    ],
  };

  test("qualification and composition join QPs matched on two hosts", () => {
    const a = qualifySnapshot("a", hostSnapshot("client", [qp(10, 20)]));
    const [relation] = a.rdma_relations!;
    expect(relation.id).toBe("a@rdma:ib0/1:10");
    expect(relation.endpoint.host).toBe("a");
    expect(
      relation.endpoint.owner.kind === "process" &&
        relation.endpoint.owner.process_id.host,
    ).toBe("a");
    expect(
      relation.cm!.owner.kind === "kernel" && relation.cm!.owner.name,
    ).toBe("rpcrdma");
    expect(a.rdma_ports![0].host).toBe("a");
    const b = qualifySnapshot("b", hostSnapshot("server", [qp(20, 10, pb)]));
    const composed = composeHosts(
      new Map([
        ["a", a],
        ["b", b],
      ]),
      links,
    );
    expect(composed.rdma_relations!.map((r) => [r.id, r.peer?.host])).toEqual([
      ["a@rdma:ib0/1:10|b@rdma:ib0/1:20", "b"],
    ]);
    expect(composed.rdma_relations![0].label).toBe("RDMA a QP 10 ↔ b QP 20");
    // Same-named devices of two hosts stay apart.
    expect(rdmaModel([], composed.rdma_ports!, new Set()).devices.size).toBe(2);
    // A link to a relation not received yet keeps the external end.
    const partial = composeHosts(new Map([["a", a]]), links);
    expect(partial.rdma_relations!.map((r) => r.id)).toEqual([
      "a@rdma:ib0/1:10",
    ]);
  });

  test("the merged view draws a QP link between host layers", () => {
    let source: TestEventSource | undefined;
    const data = new SpaceDataStore(undefined, undefined, (url) => {
      source = new TestEventSource(url);
      return source.asEventSource();
    });
    data.host = ALL_HOSTS;
    data.start();
    const hosts: HostsResponse = {
      local: "a",
      hosts: ["a", "b"].map((name, i) => ({
        name,
        local: i === 0,
        status: { state: "online", since: 0 },
        machine_id: name,
        version: "0",
        kernel: null,
        arch: "x86_64",
      })),
    };
    source!.emit("hosts", hosts);
    source!.emit("snapshot", {
      ...hostSnapshot("client", [qp(10, 20)]),
      host: "a",
      kind: "full",
      sequence: 1,
    });
    source!.emit("snapshot", {
      ...hostSnapshot("server", [qp(20, 10, pb)]),
      host: "b",
      kind: "full",
      sequence: 1,
    });
    source!.emit("links", links);
    const a = key({ ...pa, host: "a" }),
      b = key({ ...pb, host: "b" });
    expect(data.rdmaLinks.map((l) => [l.from, l.to])).toEqual([[a, b]]);
    expect(data.rdmaDevicePositions.get(rdmaDeviceId("ib0", "b"))?.z).toBe(
      -HOST_LAYER_GAP,
    );
    const scene = createSpaceScene();
    scene.rebuild(
      {
        snapshot: data.displaySnapshot,
        nodes: data.nodes,
        network: data.network,
        networkPositions: data.networkPositions,
        files: new Map(),
        netlink: new Map(),
        filePositions: new Map(),
        netlinkPositions: new Map(),
        visible: new Set(data.nodes.keys()),
        rdma: sceneRdma(data),
      },
      null,
    );
    const [link] = scene.renderView().rdmaLinkViews!;
    // From the base of the upper host's process to the roof of the lower one.
    expect(link.curve.getPoint(0).z).toBeCloseTo(0);
    expect(link.curve.getPoint(1).z).toBeCloseTo(-HOST_LAYER_GAP + 8);
    scene.dispose();
    data.stop();
  });
});
