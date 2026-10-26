// @vitest-environment jsdom
import { describe, expect, test } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import {
  HARDWARE_COLUMN,
  HARDWARE_GAP,
  HARDWARE_ROW,
  downgraded,
  hardwareLayout,
  pathLabel,
} from "../src/space/hardware.js";
import { SpaceDataStore } from "../src/space/data.js";
import { createSpaceScene } from "../src/space/scene.js";
import { createSpaceDetails } from "../src/space/details.js";
import { spaceElement } from "../src/space/dom-types.js";
import { Display } from "../src/shared/display.js";
import {
  fdEndpoint,
  fdRelation,
  processInfo,
  socketEndpoint,
} from "./support/fixtures.js";
import { TestEventSource } from "./support/mocks.js";
import type * as T from "three";
import type {
  DeviceHandle,
  HardwareDevice,
  HardwarePath,
  SystemSnapshot,
} from "../src/shared/api-types.js";

const device = (extra: Partial<HardwareDevice>): HardwareDevice => ({
  address: "",
  parent: null,
  role: "endpoint",
  kind: null,
  label: "",
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
  ...extra,
});
const link = (current: number, width: number, max = current) => ({
  current: { speed_gts: current, width },
  max: { speed_gts: max, width },
});
// This machine: a GPU and an InfiniBand HCA under different host bridges.
const devices: HardwareDevice[] = [
  device({
    address: "pci0000:00",
    role: "host_bridge",
    label: "Host bridge 0000:00",
  }),
  device({
    address: "pci0000:80",
    role: "host_bridge",
    label: "Host bridge 0000:80",
  }),
  device({
    address: "0000:00:01.1",
    parent: "pci0000:00",
    role: "root_port",
    label: "Root port 00:01.1",
    link: link(8, 16),
  }),
  device({
    address: "0000:80:03.4",
    parent: "pci0000:80",
    role: "root_port",
    label: "Root port 80:03.4",
    link: link(5, 8, 8),
  }),
  device({
    address: "0000:01:00.0",
    parent: "0000:00:01.1",
    kind: "gpu",
    label: "NVIDIA GeForce RTX 5070 Ti",
    vendor_id: "0x10de",
    device_id: "0x2c05",
    class: "0x030000",
    driver: "nvidia",
    link: link(8, 16, 32),
    bars: [{ index: 1, bytes: 256 << 20, prefetchable: true }],
    gpu: {
      vendor: "nvidia",
      model: "NVIDIA GeForce RTX 5070 Ti",
      uuid: "GPU-1",
      minor: 0,
      drm: ["card1", "renderD128"],
    },
  }),
  device({
    address: "0000:85:00.0",
    parent: "0000:80:03.4",
    kind: "rdma",
    label: "ibp133s0",
    link: link(5, 8),
    rdma: [{ name: "ibp133s0", hca_type: "MT26428", fw_ver: "2.10.720" }],
    netdevs: ["ibp133s0"],
  }),
];
const paths: HardwarePath[] = [
  {
    id: "0000:01:00.0/0000:85:00.0",
    a: "0000:01:00.0",
    b: "0000:85:00.0",
    class: "node",
    via: ["0000:00:01.1", "pci0000:00", "pci0000:80", "0000:80:03.4"],
    bottleneck: { speed_gts: 5, width: 8 },
  },
];
const pid = { pid: 42, start_time_ticks: 7 };
const handles: DeviceHandle[] = [
  {
    id: "42:7:nvidia:0000:01:00.0",
    process_id: pid,
    device: "0000:01:00.0",
    kind: "nvidia",
    fd: 19,
    fd_count: 23,
  },
  {
    id: "42:7:drm_render:0000:01:00.0",
    process_id: pid,
    device: "0000:01:00.0",
    kind: "drm_render",
    fd: 20,
    fd_count: 4,
  },
];
const snapshot: SystemSnapshot = {
  processes: [processInfo({ identity: pid, name: "chromium" })],
  fd_relations: [],
  hardware_devices: devices,
  hardware_paths: paths,
  device_handles: handles,
};

describe("hardware bay", () => {
  test("lays out PCIe trees left of the init process", () => {
    const init = { x: 1.1, y: 174 };
    const processes = [init, { x: 40, y: 174 }, { x: -100, y: 0 }];
    const positions = hardwareLayout(devices, processes, new Map(), init);
    const at = (address: string) => positions.get(address)!;
    // Devices nearest systemd, then root ports, then host bridges farther left.
    expect(at("0000:01:00.0").x).toBeCloseTo(init.x - HARDWARE_GAP);
    expect(at("0000:00:01.1").x).toBeCloseTo(
      init.x - HARDWARE_GAP - HARDWARE_ROW,
    );
    expect(at("pci0000:00").x).toBeCloseTo(
      init.x - HARDWARE_GAP - 2 * HARDWARE_ROW,
    );
    // Devices spread along y around systemd; parents sit beside their children.
    expect((at("0000:01:00.0").y + at("0000:85:00.0").y) / 2).toBe(init.y);
    expect(at("pci0000:00").y).toBe(at("0000:01:00.0").y);
    expect(at("0000:85:00.0").y - at("0000:01:00.0").y).toBeCloseTo(
      HARDWARE_COLUMN,
    );
    // A process in the way pushes the whole bay farther left.
    const crowded = hardwareLayout(
      devices,
      [...processes, { x: -9.4, y: 174 }],
      new Map(),
      init,
    );
    expect(crowded.get("pci0000:00")!.x).toBeLessThan(at("pci0000:00").x);
    for (const p of crowded.values())
      expect(Math.abs(p.x + 9.4) >= 2.7 || Math.abs(p.y - 174) >= 2.7).toBe(
        true,
      );
    // Without an anchor the bay is left of every process.
    const fallback = hardwareLayout(devices, processes);
    expect(Math.max(...[...fallback.values()].map((p) => p.x))).toBeLessThan(
      -100,
    );
    // Positions are kept until a rearrangement.
    const moved = hardwareLayout(
      devices,
      [{ x: 100, y: 100 }],
      positions,
      init,
    );
    expect(moved.get("pci0000:80")).toEqual(at("pci0000:80"));
    expect(hardwareLayout([], []).size).toBe(0);
  });

  test("links below both ends' maximum are downgraded", () => {
    const [, , rootPort, , gpu] = devices;
    // The GPU allows 32 GT/s but its root port only 8 GT/s.
    expect(downgraded(gpu, rootPort)).toBe(false);
    const faster = { ...rootPort, link: link(16, 16) };
    expect(downgraded(gpu, faster)).toBe(true);
    expect(downgraded(devices[0], undefined)).toBe(false);
    expect(pathLabel(paths[0])).toBe("NODE · Gen2 x8 (5 GT/s)");
    expect(
      [2.5, 5, 8, 16, 32, 64].map((s) => Display.pcieGeneration(s)),
    ).toEqual([1, 2, 3, 4, 5, 6]);
  });

  test("snapshots merge hardware in full and handles by ID", () => {
    let source: TestEventSource | undefined;
    const data = new SpaceDataStore(undefined, undefined, (url) => {
      source = new TestEventSource(url);
      return source.asEventSource();
    });
    data.start();
    source!.emit("snapshot", { ...snapshot, kind: "full", sequence: 1 });
    expect(data.hardwarePositions.size).toBe(devices.length);
    source!.emit("snapshot", {
      kind: "delta",
      sequence: 2,
      base_sequence: 1,
      processes: snapshot.processes,
      fd_relations: [],
      device_handles_delta: { upsert: [], remove: [handles[1].id] },
    });
    expect(data.snapshot.hardware_devices).toEqual(devices);
    expect(data.snapshot.device_handles!.map((h) => h.id)).toEqual([
      handles[0].id,
    ]);
    expect("device_handles_delta" in data.snapshot).toBe(false);
    data.stop();
  });

  test("scene draws devices and shows paths and users of a selection", () => {
    const data = new SpaceDataStore();
    data.replaceSnapshot(snapshot, true);
    const scene = createSpaceScene();
    const hardware = {
      devices,
      paths,
      handles,
      positions: data.hardwarePositions,
    };
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
        hardware,
      },
      null,
    );
    const view = scene.renderView();
    expect(
      view.hardwareViews!.map((v) => [v.device.label, v.users]),
    ).toContainEqual(["NVIDIA GeForce RTX 5070 Ti", 1]);
    const boxes = scene.root.getObjectByName("hardware-devices")!;
    expect(boxes.userData.hardware).toEqual(devices.map((d) => d.address));
    // Ribbons from each node to its upstream port, picking the child device.
    expect(
      new Set(scene.root.getObjectByName("pcie-links")!.userData.hardware),
    ).toEqual(
      new Set(["0000:00:01.1", "0000:80:03.4", "0000:01:00.0", "0000:85:00.0"]),
    );
    // Every holding process is always linked to its device, picking the device.
    const users = scene.root.getObjectByName(
      "hardware-users",
    ) as T.LineSegments;
    expect(users.geometry.getAttribute("position").count).toBe(16 * 2);
    expect(new Set(users.userData.hardwareLinks)).toEqual(
      new Set(["0000:01:00.0"]),
    );
    const selection = (extra: object) => ({
      process: null,
      connection: null,
      network: null,
      file: null,
      netlink: null,
      hardware: null,
      ...extra,
    });
    scene.updateSelection(selection({ hardware: "0000:85:00.0" }));
    expect(scene.renderView().hardwarePathViews!.map((v) => v.label)).toEqual([
      "NODE · Gen2 x8 (5 GT/s)",
    ]);
    const overlay = () =>
      (
        scene.root.getObjectByName("hardware-overlay")!.children[0] as
          T.LineSegments | undefined
      )?.geometry.getAttribute("position").count ?? 0;
    // Two ends and four nodes between them: five segments.
    expect(overlay()).toBe(10);
    scene.updateSelection(selection({ process: "42:7" }));
    expect(scene.renderView().hardwarePathViews).toEqual([]);
    // Two handles of one process to one GPU are one link of 16 segments.
    expect(overlay()).toBe(16 * 2);
    scene.updateSelection(selection({}));
    expect(overlay()).toBe(0);
    // Processes hidden by search have no links.
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
        hardware,
      },
      null,
    );
    expect(
      (
        scene.root.getObjectByName("hardware-users") as T.LineSegments
      ).geometry.getAttribute("position").count,
    ).toBe(0);
    scene.dispose();
  });

  test("details list identity, paths and processes", () => {
    document.body.innerHTML = readFileSync(
      resolve(import.meta.dirname, "../space/index.html"),
      "utf8",
    );
    const data = new SpaceDataStore();
    data.replaceSnapshot(snapshot);
    const details = createSpaceDetails(
      { connection() {}, network() {} },
      spaceElement,
    );
    details.update(
      {
        nodes: data.nodes,
        snapshot: data.displaySnapshot,
        network: data.network,
        edgeStats: new Map(),
        files: new Map(),
        netlink: new Map(),
        hardware: { devices, paths, handles },
      },
      {
        process: null,
        connection: null,
        network: null,
        file: null,
        netlink: null,
        hardware: "0000:01:00.0",
      },
    );
    expect(spaceElement("connection-label").textContent).toBe(
      "NVIDIA GeForce RTX 5070 Ti",
    );
    expect(spaceElement("connection-state").textContent).toBe(
      "GPU · 0000:01:00.0",
    );
    expect(spaceElement("connection-facts").textContent).toBe(
      "PCIe Gen3 x16 (8 GT/s) · max Gen5 x16 (32 GT/s)",
    );
    const text = spaceElement("connection-endpoints").textContent;
    expect(text).toContain("BAR 1: 256 MiB (prefetchable)");
    expect(text).toContain("GPU nvidia · GPU-1 · card1, renderD128");
    expect(text).toContain("Path to ibp133s0");
    expect(text).toContain(
      "via 0000:00:01.1 → pci0000:00 → pci0000:80 → 0000:80:03.4",
    );
    expect(text).toContain("chromium");
    expect(text).toContain(
      "PID 42 · nvidia ×23 (FD 19), drm_render ×4 (FD 20)",
    );
  });

  test("network destinations over IPoIB show the remote GID", () => {
    document.body.innerHTML = readFileSync(
      resolve(import.meta.dirname, "../space/index.html"),
      "utf8",
    );
    const gid = "fe80:0000:0000:0000:f452:1403:0080:de51";
    const relation = fdRelation({
      id: "nfs",
      endpoint: fdEndpoint({
        process_id: pid,
        kind: "socket",
        resource: {
          kind: "socket",
          device: { major: 0, minor: 8 },
          inode: "9",
        },
      }),
      socket: socketEndpoint({
        network_peer: true,
        local: { ip: "10.10.10.2", port: 700 },
        remote: { ip: "10.10.10.1", port: 2049 },
        remote_gid: gid,
      }),
    });
    const data = new SpaceDataStore();
    data.replaceSnapshot({ ...snapshot, fd_relations: [relation] });
    const [group] = data.network.values();
    expect(group.label).toBe("TCP 10.10.10.1:2049 · IB ×1");
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
    };
    const none = { process: null, connection: null, file: null, netlink: null };
    details.update(input, { ...none, network: group.id });
    expect(spaceElement("connection-endpoints").textContent).toContain(
      `InfiniBand GID ${gid} (IPoIB neighbor)`,
    );
    details.update(input, { ...none, network: null, connection: "nfs" });
    expect(spaceElement("connection-endpoints").textContent).toContain(
      `InfiniBand GID ${gid}`,
    );
  });
});
