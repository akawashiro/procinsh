import { test, expect } from "vitest";
import {
  readFileSync,
  mkdtempSync,
  truncateSync,
  writeFileSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  SpaceRecorder,
  SpaceReplay,
  parseRecording,
} from "../src/space/recording.js";
import { SpaceDataStore } from "../src/space/data.js";
import {
  processInfo,
  spaceActivity,
  memoryMap,
  cpuActivity,
} from "./support/fixtures.js";
import {
  assertPagesAssets,
  PAGES_MAX_ASSET_BYTES,
} from "../vite.replay.config.js";
import { TestEventSource } from "./support/mocks.js";

const sample = () =>
  parseRecording(
    readFileSync(
      new URL("../replay-public/recordings/demo.json", import.meta.url),
      "utf8",
    ),
  );
test("recording starts midstream with complete merged maps and preserves event order", () => {
  let now = 1000;
  const recorder = new SpaceRecorder(() => now);
  let source: TestEventSource;
  const data = new SpaceDataStore(
    undefined,
    undefined,
    (url) => {
      source = new TestEventSource(url);
      return source.asEventSource();
    },
    undefined,
    (event) => recorder.capture(event),
  );
  data.start();
  source!.emit("snapshot", {
    kind: "full",
    sequence: 1,
    processes: [processInfo({ maps: [memoryMap()] })],
    fd_relations: [],
  });
  const { maps: _maps, ...process } = processInfo();
  source!.emit("snapshot", {
    kind: "delta",
    sequence: 2,
    base_sequence: 1,
    processes: [process],
    fd_relations_delta: { upsert: [], remove: [] },
  });
  recorder.start(data.snapshot);
  const baseline = structuredClone(data.snapshot);
  now = 1200;
  source!.emit(
    "activity",
    spaceActivity({ cpu: [cpuActivity({ runtime_ns: 100 })] }),
  );
  source!.emit("snapshot", {
    kind: "delta",
    sequence: 3,
    base_sequence: 2,
    processes: [process],
    fd_relations_delta: { upsert: [], remove: [] },
  });
  now = 1500;
  recorder.stop("Connection interrupted");
  source!.emit("activity", spaceActivity());
  const result = parseRecording(JSON.stringify(recorder.recording));
  expect(result.initial_snapshot).toEqual(baseline);
  expect(result.initial_snapshot.processes[0].maps).toEqual([memoryMap()]);
  expect(result.events.map((e) => [e.type, e.at_ms])).toEqual([
    ["activity", 200],
    ["snapshot", 200],
  ]);
  expect(result.duration_ms).toBe(500);
  expect(result.end_reason).toBe("Connection interrupted");
  data.stop();
});

test("virtual clock controls slow playback, pause, seek and event stepping", () => {
  const recording = sample();
  const times: number[] = [];
  let clock = -1;
  const replay = new SpaceReplay(
    recording,
    () => {
      times.length = 0;
    },
    (event) => {
      expect(clock).toBe(event.at_ms);
      times.push(event.at_ms);
    },
    (time) => {
      clock = time;
    },
  );
  replay.speed = 0.25;
  replay.playing = true;
  replay.tick(1000);
  expect(replay.time).toBe(250);
  expect(times).toEqual([0, 100, 200]);
  replay.playing = false;
  replay.tick(3000);
  expect(replay.time).toBe(250);
  replay.step(-1);
  expect(replay.time).toBe(200);
  replay.step(1);
  expect(replay.time).toBe(300);
  replay.seek(0);
  expect(times).toEqual([0]);
  replay.seek(1e9);
  expect(replay.time).toBe(recording.duration_ms);
  expect(clock).toBe(recording.duration_ms);
});

test("shared validator rejects invalid metadata, ordering, and nested payloads", () => {
  expect(() => parseRecording("not json")).toThrow("valid JSON");
  for (const [mutate, message] of [
    [
      (v: any) => (v.format_version = 2),
      "Unsupported recording format version",
    ],
    [(v: any) => (v.view_type = "memory"), "SPACE recording"],
    [(v: any) => (v.started_at = "bad"), "started_at"],
    [(v: any) => (v.events[1].at_ms = -1), "at_ms"],
    [(v: any) => (v.events[1].at_ms = v.duration_ms + 1), "within duration"],
    [(v: any) => (v.events[2].at_ms = 1), "ordered"],
    [
      (v: any) => (v.initial_snapshot.processes[0].maps[0].start = "oops"),
      "start",
    ],
    [(v: any) => (v.events[0].data.cpu[0].process_id = null), "process_id"],
    [
      (v: any) => (v.initial_snapshot.fd_relations[0].endpoint.resource = null),
      "resource",
    ],
  ] as const) {
    const recording = sample();
    mutate(recording);
    expect(() => parseRecording(JSON.stringify(recording))).toThrow(message);
  }
});

test("public demo is self-contained and contains synthetic values only", () => {
  const recording = sample();
  expect(recording.events.length).toBeGreaterThan(100);
  expect(
    recording.initial_snapshot.processes.every((p) => p.username === "demo"),
  ).toBe(true);
  expect(recording.end_reason).toContain("Synthetic sanitized demo");
});

test("Cloudflare Pages Free build guard accepts the boundary and rejects oversized files", () => {
  const directory = mkdtempSync(join(tmpdir(), "procinsh-pages-limit-"));
  try {
    const path = join(directory, "demo.json");
    writeFileSync(path, "");
    truncateSync(path, PAGES_MAX_ASSET_BYTES);
    expect(() => assertPagesAssets(directory)).not.toThrow();
    truncateSync(path, PAGES_MAX_ASSET_BYTES + 1);
    expect(() => assertPagesAssets(directory)).toThrow(/25 MiB.*demo.json/);
  } finally {
    rmSync(directory, { recursive: true });
  }
});

test("loop playback wraps the virtual clock and rebuilds state without losing overflow", () => {
  const recording = sample();
  let resets = 0;
  let clock = -1;
  const events: number[] = [];
  const replay = new SpaceReplay(
    recording,
    () => {
      resets++;
      events.length = 0;
    },
    (event) => {
      events.push(event.at_ms);
    },
    (time) => {
      clock = time;
    },
  );
  replay.loop = true;
  replay.speed = 0.5;
  replay.seek(recording.duration_ms - 100);
  replay.playing = true;
  replay.tick(600);
  expect(replay.time).toBe(200);
  expect(clock).toBe(200);
  expect(events).toEqual([0, 100, 200]);
  expect(resets).toBe(3);
  expect(replay.playing).toBe(true);
  replay.tick(recording.duration_ms * 4);
  expect(replay.time).toBe(200);
  replay.seek(recording.duration_ms - 100);
  replay.tick(200);
  expect(replay.time).toBe(0);
  expect(replay.playing).toBe(true);
  replay.playing = false;
  replay.tick(1000);
  expect(replay.time).toBe(0);
  replay.loop = false;
  replay.playing = true;
  replay.tick(recording.duration_ms * 2);
  expect(replay.time).toBe(recording.duration_ms);
  expect(replay.playing).toBe(false);
});

test("looping a zero-duration recording stops cleanly", () => {
  const recording = sample();
  recording.duration_ms = 0;
  recording.events = [];
  const replay = new SpaceReplay(
    recording,
    () => {},
    () => {},
    () => {},
  );
  replay.loop = true;
  replay.playing = true;
  replay.tick(1000);
  expect(replay.time).toBe(0);
  expect(replay.playing).toBe(false);
});

test("replay stops, seeks and loops at the final fractional event time instead of the recording stop time", () => {
  const recording = sample();
  recording.duration_ms = 500;
  recording.events = [
    { at_ms: 25.125, type: "activity", data: spaceActivity() },
    { at_ms: 103.375, type: "activity", data: spaceActivity() },
  ];
  const applied: number[] = [];
  let clock = -1;
  const replay = new SpaceReplay(
    recording,
    () => {
      applied.length = 0;
    },
    (event) => {
      applied.push(event.at_ms);
    },
    (time) => {
      clock = time;
    },
  );
  expect(replay.duration).toBe(103.375);
  replay.playing = true;
  replay.tick(200);
  expect(replay.time).toBe(103.375);
  expect(clock).toBe(103.375);
  expect(replay.playing).toBe(false);
  expect(applied).toEqual([25.125, 103.375]);
  replay.seek(500);
  expect(replay.time).toBe(103.375);
  replay.step(-1);
  expect(replay.time).toBe(25.125);
  replay.step(1);
  expect(replay.time).toBe(103.375);
  replay.loop = true;
  replay.playing = true;
  replay.tick(30);
  expect(replay.time).toBe(30);
  expect(clock).toBe(30);
  expect(applied).toEqual([25.125]);
  expect(replay.playing).toBe(true);
  expect(recording.duration_ms).toBe(500);
});

test("a recording without events has no empty playback tail", () => {
  const recording = sample();
  recording.duration_ms = 500;
  recording.events = [];
  const replay = new SpaceReplay(
    recording,
    () => {},
    () => {},
    () => {},
  );
  expect(replay.duration).toBe(0);
  replay.seek(500);
  expect(replay.time).toBe(0);
  replay.loop = true;
  replay.playing = true;
  replay.tick(100);
  expect(replay.playing).toBe(false);
});
