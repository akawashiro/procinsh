// Pure hardware bay layout and labels: PCIe trees beside the init process (-x).

import { Display } from "../shared/display.js";
import type {
  DeviceHandle,
  HardwareDevice,
  HardwarePath,
  PcieLink,
} from "../shared/api-types.js";
import type { Position } from "./types.js";

/** Distance from the anchor process to the device column. */
export const HARDWARE_GAP = 3.5;
/** Distance between tree levels, from devices toward host bridges. */
export const HARDWARE_ROW = 3.5;
/** Distance between neighboring devices in a column. */
export const HARDWARE_COLUMN = 4.8;
/** Half the distance kept between node and process centers: 2.2 wide boxes and an aisle. */
const CLEARANCE = 1.35;

/** Usable bandwidth in GB/s after line encoding. */
export const pcieBandwidth = (link: PcieLink) =>
  (link.speed_gts * (link.speed_gts <= 5 ? 0.8 : 128 / 130) * link.width) / 8;

/**
 * Lays out PCIe trees to the left (-x) of `anchor`, the init process (systemd):
 * devices in the column nearest it, spread along y around it, and bridges up to
 * the host bridges farther left, each parent centered beside its children. The
 * bay moves farther left until it overlaps no process. Without an anchor it is
 * placed left of all processes. Devices keep previous positions until the layout
 * is rearranged.
 */
export function hardwareLayout(
  devices: readonly HardwareDevice[],
  processes: Iterable<Pick<Position, "x" | "y">>,
  previous: ReadonlyMap<string, Position> = new Map(),
  anchor?: Pick<Position, "x" | "y">,
) {
  const result = new Map<string, Position>();
  if (!devices.length) return result;
  if (devices.every((d) => previous.has(d.address))) {
    for (const d of devices) result.set(d.address, previous.get(d.address)!);
    return result;
  }
  const all = [...processes];
  const origin = anchor ?? {
    x: all.length ? Math.min(...all.map((p) => p.x)) : 0,
    y: all.length ? all.reduce((sum, p) => sum + p.y, 0) / all.length : 0,
  };
  const children = new Map<string | null, HardwareDevice[]>();
  const known = new Set(devices.map((d) => d.address));
  for (const d of devices) {
    const parent = d.parent !== null && known.has(d.parent) ? d.parent : null;
    children.set(parent, [...(children.get(parent) ?? []), d]);
  }
  for (const list of children.values())
    list.sort((a, b) => a.address.localeCompare(b.address));
  // Depth below the root bridge and the deepest leaf decide each column.
  const depth = new Map<string, number>();
  const visit = (parent: string | null, level: number) => {
    for (const d of children.get(parent) ?? []) {
      depth.set(d.address, level);
      visit(d.address, level + 1);
    }
  };
  visit(null, 0);
  const deepest = Math.max(...depth.values());
  let row = 0;
  const offset = new Map<string, number>();
  const place = (parent: string | null) => {
    for (const d of children.get(parent) ?? []) {
      const below = children.get(d.address) ?? [];
      if (!below.length) offset.set(d.address, row++ * HARDWARE_COLUMN);
      else {
        place(d.address);
        const ys = below.map((c) => offset.get(c.address)!);
        offset.set(d.address, (Math.min(...ys) + Math.max(...ys)) / 2);
      }
    }
  };
  place(null);
  const spread = (row - 1) * HARDWARE_COLUMN;
  const relative = devices.map((d) => ({
    address: d.address,
    x: -(
      HARDWARE_GAP +
      ((children.get(d.address) ?? []).length
        ? (deepest - depth.get(d.address)!) * HARDWARE_ROW
        : 0)
    ),
    y: offset.get(d.address)! - spread / 2,
  }));
  const overlaps = (shift: number) =>
    relative.some((n) =>
      all.some(
        (p) =>
          Math.abs(origin.x + shift + n.x - p.x) < 2 * CLEARANCE &&
          Math.abs(origin.y + n.y - p.y) < 2 * CLEARANCE,
      ),
    );
  let shift = 0;
  for (let step = 0; step < 200 && overlaps(shift); step++)
    shift -= HARDWARE_ROW;
  for (const n of relative)
    result.set(n.address, {
      x: origin.x + shift + n.x,
      y: origin.y + n.y,
      z: 0,
    });
  return result;
}

/** The link runs slower than both ends allow, such as an idle GPU or a narrow slot. */
export function downgraded(
  device: HardwareDevice,
  parent: HardwareDevice | undefined,
) {
  const current = device.link?.current,
    max = device.link?.max;
  if (!current || !max) return false;
  const limit = parent?.link?.max;
  const speed = Math.min(max.speed_gts, limit?.speed_gts ?? Infinity),
    width = Math.min(max.width, limit?.width ?? Infinity);
  return current.speed_gts < speed || current.width < width;
}

export const pathLabel = (path: HardwarePath) =>
  `${path.class.toUpperCase()}${path.bottleneck ? ` · ${Display.pcieLink(path.bottleneck)}` : ""}`;

/** Users of each device, as process keys mapped to their handles. */
export function deviceUsers(
  handles: readonly DeviceHandle[],
  key: (handle: DeviceHandle) => string,
) {
  const users = new Map<string, Map<string, DeviceHandle[]>>();
  for (const handle of handles) {
    const byProcess = users.get(handle.device) ?? new Map();
    byProcess.set(key(handle), [...(byProcess.get(key(handle)) ?? []), handle]);
    users.set(handle.device, byProcess);
  }
  return users;
}
