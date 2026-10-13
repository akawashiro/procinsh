// @vitest-environment jsdom
import { test, vi, afterEach } from "vitest";
import assert from "node:assert/strict";
import * as T from "three";
import {
  decodeX86_64,
  ProcessInspectionCard,
  computeCubeHeight,
  computeHeightForLines,
  DEFAULT_SLICES_PER_PAGE,
  MIN_SLICES_PER_PAGE,
  MAX_SLICES_PER_PAGE,
} from "../src/space/inspection-card.js";
import { findNearestRegion } from "../src/space/selection.js";
import type { Region } from "../src/space/types.js";
import type { GlyphAtlas } from "@glyph3d/core";

afterEach(() => {
  vi.restoreAllMocks();
  try {
    localStorage.clear();
  } catch {}
});

test("decodeX86_64 instruction decoder", () => {
  // Test basic instructions
  const bytes = new Uint8Array([
    0x55, // push rbp
    0x48,
    0x89,
    0xe5, // mov rbp, rsp
    0x48,
    0x83,
    0xec,
    0x20, // sub rsp, 0x20
    0x31,
    0xc0, // xor eax, eax
    0x90, // nop
    0x0f,
    0x05, // syscall
    0xc3, // ret
    0xe8,
    0x0a,
    0x00,
    0x00,
    0x00, // call +10
    0x74,
    0x02, // jz +2
    0xcc, // int3
  ]);

  const baseAddr = 0x55d7f1e00000n;
  const insns = decodeX86_64(bytes, baseAddr);

  assert.equal(insns.length, 10);

  assert.equal(insns[0].mnemonic, "push");
  assert.equal(insns[0].operands, "rbp");
  assert.equal(insns[0].address, baseAddr);

  assert.equal(insns[1].mnemonic, "mov");
  assert.equal(insns[1].operands, "rbp, rsp");
  assert.equal(insns[1].address, baseAddr + 1n);

  assert.equal(insns[2].mnemonic, "sub");
  assert.equal(insns[2].operands, "rsp, 0x20");

  assert.equal(insns[3].mnemonic, "xor");
  assert.equal(insns[3].operands, "eax, eax");

  assert.equal(insns[4].mnemonic, "nop");
  assert.equal(insns[5].mnemonic, "syscall");
  assert.equal(insns[6].mnemonic, "ret");

  assert.equal(insns[7].mnemonic, "call");
  assert.equal(
    insns[7].operands,
    `0x${(baseAddr + 14n + 5n + 10n).toString(16)}`,
  );

  assert.equal(insns[8].mnemonic, "jz");
  assert.equal(insns[9].mnemonic, "int3");
});

test("ProcessInspectionCard anchor, billboarding, and mode toggling", () => {
  // Mock GlyphAtlas handle
  const mockAtlas = {
    fontFamily: "monospace",
    fontSize: 48,
    atlasSize: 2048,
    dispose() {},
  } as unknown as GlyphAtlas;

  const card = new ProcessInspectionCard(mockAtlas);
  assert.ok(card instanceof T.Group);
  assert.equal(card.visible, false);
  assert.equal(card.mode, "hex");

  // Mode toggling
  card.toggleMode();
  assert.equal(card.mode, "disasm");
  card.toggleMode();
  assert.equal(card.mode, "hex");

  // Anchor and billboarding updates
  // 3D Memory Cube Anchor and perspective orientation
  const camera = new T.PerspectiveCamera(45, 800 / 600, 0.1, 100);
  camera.position.set(65, -85, 70);
  camera.lookAt(0, 0, 0);
  camera.updateMatrixWorld();

  const hitPoint = new T.Vector3(5, 2, 4);
  card.updateAnchor(hitPoint, camera);

  // Cube position is in 3D world space offset from hitPoint
  assert.ok(card.cubeGroup.position.distanceTo(hitPoint) > 1.0);

  // Leader line geometry is populated with hitPoint on the pillar and left anchor on the cube
  const posAttr = card.leaderLine.geometry.attributes.position;
  assert.equal(posAttr.getX(0), 5);
  assert.equal(posAttr.getY(0), 2);
  assert.equal(posAttr.getZ(0), 4);
  assert.ok(posAttr.getX(1) !== 0 || posAttr.getY(1) !== 0);

  // Check that front normal (local Z transformed to world) has gentle ~18° perspective angle to camera
  const normalWorld = new T.Vector3(0, 0, 1).applyQuaternion(
    card.cubeGroup.quaternion,
  );
  const camForward = new T.Vector3(0, 0, -1).applyQuaternion(camera.quaternion);
  const toCam = camForward.clone().negate();
  const angleDeg = T.MathUtils.radToDeg(normalWorld.angleTo(toCam));
  assert.ok(
    angleDeg >= 15 && angleDeg <= 25,
    `Angle should be ~18 deg, got ${angleDeg}`,
  );
  assert.ok(normalWorld.dot(toCam) > 0.9, "Normal should face toward camera");

  // Locking mechanism: lock() pins the cube in 3D world space
  card.visible = true;
  // @ts-expect-error test mock node
  card.currentProcessNode = { identity: { pid: 101, start_time_ticks: 1 } };
  card.lock();
  assert.equal(card.locked, true);
  assert.equal(card.isLocked(), true);

  const lockedPos = card.cubeGroup.position.clone();
  // Move hitPoint: when locked, updateAnchor MUST NOT move the cube!
  const newHit = new T.Vector3(10, 10, 8);
  card.updateAnchor(newHit, camera);
  assert.equal(card.cubeGroup.position.x, lockedPos.x);
  assert.equal(card.cubeGroup.position.y, lockedPos.y);
  assert.equal(card.cubeGroup.position.z, lockedPos.z);

  // Unlock releases the pin and hides
  card.unlock();
  assert.equal(card.locked, false);
  assert.equal(card.visible, false);

  // Paging controls
  card.totalPages = 5;
  card.pageIndex = 0;
  // @ts-expect-error test mock region
  card.currentRegion = {
    start: "0x1000",
    end: "0x3000",
    readable: true,
    z: 0,
    h: 2,
  };
  card.visible = true;
  card.currentBuffer = new Uint8Array(5 * 512);
  card.currentBufferLen = 5 * 512;
  card.scrollPages(1);
  assert.equal(card.pageIndex, 1);
  card.scrollPages(2);
  assert.equal(card.pageIndex, 3);
  card.scrollPages(-1);
  assert.equal(card.pageIndex, 2);

  // hitTest verification
  const ray = new T.Raycaster();
  const cubePos = card.cubeGroup.position.clone();
  ray.set(
    new T.Vector3(cubePos.x, cubePos.y, cubePos.z + 10),
    new T.Vector3(0, 0, -1),
  );
  assert.equal(card.hitTest(ray), true);

  const missRay = new T.Raycaster();
  missRay.set(new T.Vector3(999, 999, 999), new T.Vector3(0, 0, -1));
  assert.equal(card.hitTest(missRay), false);

  card.visible = false;
  assert.equal(card.hitTest(ray), false);

  // keepAlive verification
  card.keepAlive();

  card.dispose();
});

test("findNearestRegion resolves gaps and boundaries stably", () => {
  const regions: Region[] = [
    {
      start: "0x1000",
      end: "0x2000",
      readable: true,
      writable: false,
      executable: true,
      private: true,
      file_offset: "0",
      device: { major: 0, minor: 0 },
      inode: "0",
      pathname: "/bin/test",
      z: 0.0,
      h: 2.0,
    },
    // Visual gap from z = 2.0 to z = 2.5
    {
      start: "0x2000",
      end: "0x3000",
      readable: true,
      writable: true,
      executable: false,
      private: true,
      file_offset: "0",
      device: { major: 0, minor: 0 },
      inode: "0",
      pathname: "[heap]",
      z: 2.5,
      h: 3.0,
    },
    // Visual gap from z = 5.5 to z = 6.0
    {
      start: "0x7fff0000",
      end: "0x7fff1000",
      readable: true,
      writable: true,
      executable: false,
      private: true,
      file_offset: "0",
      device: { major: 0, minor: 0 },
      inode: "0",
      pathname: "[stack]",
      z: 6.0,
      h: 2.0,
    },
  ];

  // Inside first region
  assert.equal(findNearestRegion(regions, 1.0)?.pathname, "/bin/test");

  // In the gap between region 0 and region 1: closer to region 0 (z = 2.1)
  assert.equal(findNearestRegion(regions, 2.1)?.pathname, "/bin/test");

  // In the gap between region 0 and region 1: closer to region 1 (z = 2.4)
  assert.equal(findNearestRegion(regions, 2.4)?.pathname, "[heap]");

  // In the gap between region 1 and region 2: closer to region 2 (z = 5.8)
  assert.equal(findNearestRegion(regions, 5.8)?.pathname, "[stack]");

  // Outside bounds below 0: clamped to first
  assert.equal(findNearestRegion(regions, -1.0)?.pathname, "/bin/test");

  // Outside bounds above 8: clamped to last
  assert.equal(findNearestRegion(regions, 10.0)?.pathname, "[stack]");

  // Empty regions array returns undefined
  assert.equal(findNearestRegion([], 4.0), undefined);
});

test("ProcessInspectionCard leave grace period preserves visibility across brief gap", () => {
  vi.useFakeTimers();
  const mockAtlas = {
    fontFamily: "monospace",
    fontSize: 48,
    atlasSize: 2048,
    dispose() {},
  } as unknown as GlyphAtlas;

  const card = new ProcessInspectionCard(mockAtlas);
  const camera = new T.PerspectiveCamera(45, 1, 0.1, 100);

  // Set card visible
  card.visible = true;

  // Pointer leaves (target = null)
  card.handleHover(null, camera);

  // Still visible during grace period (e.g. at 50ms)
  vi.advanceTimersByTime(50);
  assert.equal(card.visible, true);

  // After grace period expires (at 200ms total), card hides
  vi.advanceTimersByTime(150);
  assert.equal(card.visible, false);

  card.dispose();
  vi.useRealTimers();
});

test("ProcessInspectionCard multi-slice region concatenation into grid pages", async () => {
  const mockAtlas = {
    fontFamily: "monospace",
    fontSize: 48,
    atlasSize: 2048,
    dispose() {},
  } as unknown as GlyphAtlas;

  const card = new ProcessInspectionCard(mockAtlas);
  const camera = new T.PerspectiveCamera(45, 1, 0.1, 100);

  const regions: Region[] = [
    {
      start: "0x1000",
      end: "0x2000",
      readable: true,
      writable: false,
      executable: true,
      private: true,
      file_offset: "0",
      device: { major: 0, minor: 0 },
      inode: "0",
      pathname: "/bin/app",
      z: 0.0,
      h: 1.0,
    },
    {
      start: "0x2000",
      end: "0x3000",
      readable: true,
      writable: false,
      executable: false,
      private: true,
      file_offset: "0",
      device: { major: 0, minor: 0 },
      inode: "0",
      pathname: "/bin/app",
      z: 1.0,
      h: 1.0,
    },
    {
      start: "0x3000",
      end: "0x4000",
      readable: false,
      writable: false,
      executable: false,
      private: true,
      file_offset: "0",
      device: { major: 0, minor: 0 },
      inode: "0",
      pathname: "",
      z: 2.0,
      h: 1.0,
    },
    {
      start: "0x4000",
      end: "0x5000",
      readable: true,
      writable: true,
      executable: false,
      private: true,
      file_offset: "0",
      device: { major: 0, minor: 0 },
      inode: "0",
      pathname: "[heap]",
      z: 3.0,
      h: 1.0,
    },
    {
      start: "0x7000",
      end: "0x8000",
      readable: true,
      writable: true,
      executable: false,
      private: true,
      file_offset: "0",
      device: { major: 0, minor: 0 },
      inode: "0",
      pathname: "[stack]",
      z: 4.0,
      h: 1.0,
    },
  ];

  const mockNode = {
    identity: { pid: 42, start_time_ticks: 100 },
    name: "test-proc",
    pos: new T.Vector3(0, 0, 0),
    regions,
  } as any;

  // Hovering on region 3 (heap) should calculate total pages and jump to page index 1
  card.handleHover(
    {
      process: mockNode,
      region: regions[3],
      hitPoint: new T.Vector3(0, 0, 3.5),
    },
    camera,
  );
  assert.equal(card.totalPages, 2); // 5 regions / 3 per page = 2 pages
  assert.equal(card.pageIndex, 1); // region index 3 is on page 1 (slices 3, 4)
  assert.equal(card.currentHoveredRegion, regions[3]);

  // Scrolling backwards to page 0
  card.scrollPages(-1);
  assert.equal(card.pageIndex, 0); // page 0 has slices 0, 1, 2

  // Scrolling forwards to page 1
  card.scrollPages(1);
  assert.equal(card.pageIndex, 1);

  // Clamping at boundaries
  card.scrollPages(10);
  assert.equal(card.pageIndex, 1); // clamped to totalPages - 1
  card.scrollPages(-10);
  assert.equal(card.pageIndex, 0); // clamped to 0

  card.dispose();
});

test("ProcessInspectionCard slicesPerPage adjustment and dynamic height calculation", async () => {
  const mockAtlas = {
    fontFamily: "monospace",
    fontSize: 48,
    atlasSize: 2048,
    dispose() {},
  } as unknown as GlyphAtlas;

  const card = new ProcessInspectionCard(mockAtlas);
  assert.equal(card.slicesPerPage, DEFAULT_SLICES_PER_PAGE);
  assert.equal(card.showMappingRows, true);

  const initialHeight = card.currentCubeHeight;
  assert.ok(initialHeight >= 3.4 && initialHeight <= 3.6);

  // Increase slices per page
  await card.increaseSlicesPerPage();
  assert.equal(card.slicesPerPage, 4);
  assert.ok(card.currentCubeHeight > initialHeight);

  // Increase again
  await card.increaseSlicesPerPage();
  assert.equal(card.slicesPerPage, 5);

  // Decrease slices per page
  await card.decreaseSlicesPerPage();
  assert.equal(card.slicesPerPage, 4);

  // Clamp at max (16)
  await card.setSlicesPerPage(100);
  assert.equal(card.slicesPerPage, MAX_SLICES_PER_PAGE);

  // Clamp at min (1)
  await card.setSlicesPerPage(0);
  assert.equal(card.slicesPerPage, MIN_SLICES_PER_PAGE);

  // computeCubeHeight sanity checks
  const h1 = computeCubeHeight(1, true);
  const h3 = computeCubeHeight(3, true);
  const h6 = computeCubeHeight(6, true);
  assert.ok(h1 < h3 && h3 < h6);

  // Height without mapping rows should be smaller than with mapping rows for same slices
  const h3NoMaps = computeCubeHeight(3, false);
  assert.ok(h3NoMaps < h3);

  // Toggle mapping rows
  await card.toggleMappingRows();
  assert.equal(card.showMappingRows, false);
  await card.toggleMappingRows();
  assert.equal(card.showMappingRows, true);

  card.dispose();
});

test("ProcessInspectionCard preserves viewing slice focus across slicesPerPage changes", async () => {
  const mockAtlas = {
    fontFamily: "monospace",
    fontSize: 48,
    atlasSize: 2048,
    dispose() {},
  } as unknown as GlyphAtlas;

  const card = new ProcessInspectionCard(mockAtlas);
  const camera = new T.PerspectiveCamera(45, 1, 0.1, 100);

  const regions: Region[] = Array.from({ length: 12 }, (_, i) => ({
    start: `0x${(i * 0x1000).toString(16)}`,
    end: `0x${((i + 1) * 0x1000).toString(16)}`,
    readable: true,
    writable: false,
    executable: false,
    private: true,
    file_offset: "0",
    device: { major: 0, minor: 0 },
    inode: "0",
    pathname: `/bin/lib${i}.so`,
    z: i * 1.0,
    h: 1.0,
  }));

  const mockNode = {
    identity: { pid: 99, start_time_ticks: 1 },
    name: "focus-proc",
    pos: new T.Vector3(0, 0, 0),
    regions,
  } as any;

  // Hover region 7 (index 7). With 3 slices per page: page index 2 (slices 6, 7, 8)
  card.handleHover(
    {
      process: mockNode,
      region: regions[7],
      hitPoint: new T.Vector3(0, 0, 7.0),
    },
    camera,
  );
  assert.equal(card.pageIndex, 2);
  assert.equal(card.totalPages, 4);

  // Change slicesPerPage to 4. Region 7 should now be on page Math.floor(7 / 4) = 1 (slices 4, 5, 6, 7)
  await card.setSlicesPerPage(4);
  assert.equal(card.pageIndex, 1);
  assert.equal(card.totalPages, 3);

  // Change slicesPerPage to 6. Region 7 should now be on page Math.floor(7 / 6) = 1 (slices 6..11)
  await card.setSlicesPerPage(6);
  assert.equal(card.pageIndex, 1);
  assert.equal(card.totalPages, 2);

  // Change slicesPerPage to 8. Region 7 should now be on page Math.floor(7 / 8) = 0 (slices 0..7)
  await card.setSlicesPerPage(8);
  assert.equal(card.pageIndex, 0);
  assert.equal(card.totalPages, 2);

  card.dispose();
});

test("ProcessInspectionCard dynamically sizes cube height to visible lines on unreadable/short blocks", async () => {
  const mockAtlas = {
    fontFamily: "monospace",
    fontSize: 48,
    atlasSize: 2048,
    dispose() {},
  } as unknown as GlyphAtlas;

  const card = new ProcessInspectionCard(mockAtlas);
  const camera = new T.PerspectiveCamera(45, 1, 0.1, 100);

  // 4 unreadable/guard regions
  const guardRegions: Region[] = Array.from({ length: 4 }, (_, i) => ({
    start: `0x${(i * 0x1000).toString(16)}`,
    end: `0x${((i + 1) * 0x1000).toString(16)}`,
    readable: false,
    writable: false,
    executable: false,
    private: true,
    file_offset: "0",
    device: { major: 0, minor: 0 },
    inode: "0",
    pathname: "[guard]",
    z: i * 1.0,
    h: 1.0,
  }));

  const mockNode = {
    identity: { pid: 42, start_time_ticks: 1 },
    name: "test-proc",
    pos: new T.Vector3(0, 0, 0),
    regions: guardRegions,
  } as any;

  await card.setSlicesPerPage(4);
  card.handleHover(
    {
      process: mockNode,
      region: guardRegions[0],
      hitPoint: new T.Vector3(0, 0, 0),
    },
    camera,
  );

  // Wait for debounce render
  await new Promise((r) => setTimeout(r, 80));

  // The guard slices only have ~3 lines each instead of 9 lines each.
  // The height should match computeHeightForLines for the actual lines, significantly smaller than theoretical 4 * 9 rows
  const theoreticalFullHeight = computeCubeHeight(4, true);
  assert.ok(
    card.currentCubeHeight < theoreticalFullHeight,
    `Current height ${card.currentCubeHeight} should be smaller than full theoretical height ${theoreticalFullHeight}`,
  );

  // Test showNotice shrinks cube to notice lines
  await card.showNotice("Notice Title", "Short message", ["Line 1"]);
  const noticeHeight = card.currentCubeHeight;
  assert.ok(
    noticeHeight < theoreticalFullHeight,
    `Notice height ${noticeHeight} should shrink to fit notice lines`,
  );
  assert.ok(noticeHeight >= 1.8 && noticeHeight <= 2.0);

  card.dispose();
});
