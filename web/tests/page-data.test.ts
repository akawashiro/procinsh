import { test, vi } from "vitest";
import assert from "node:assert/strict";
import { ListDataStore } from "../src/list/data.js";
import { ProcessDataStore } from "../src/process/data.js";
import { ThreadSelection } from "../src/process/selection.js";
import {
  matchingEnvironment,
  matchingDescriptors,
} from "../src/process/search.js";

import type {
  FileDescriptors,
  ProcessSummary,
} from "../src/shared/api-types.js";
import { errorMessage } from "../src/shared/api.js";
import {
  processSummary,
  threadSample,
  threadObservation,
  processObservation,
  targetInfo,
} from "./support/fixtures.js";
import { deferred, mockApi, TestEventSource } from "./support/mocks.js";

test("page-data regression", async () => {
  // Exercise page state without a browser or live server.

  const flush = () => new Promise((resolve) => setImmediate(resolve));
  const timers = new Map<
    number,
    { fn: () => unknown; ms: number | undefined }
  >();
  let timerId = 0;
  vi.stubGlobal("setInterval", (fn: () => unknown, ms?: number) => {
    const id = ++timerId;
    timers.set(id, { fn, ms });
    return id;
  });
  vi.stubGlobal("clearInterval", (id: number) => timers.delete(id));
  const id = { pid: 10, start_time_ticks: 100 };
  const process = (
    pid: number,
    name: string,
    cpu_percent: number | null,
    rss_bytes: number,
  ) =>
    processSummary({
      identity: { ...id, pid },
      name,
      cpu_percent,
      rss_bytes,
      command_line: ["--worker"],
    });
  const processes = [
    process(10, "main", null, 5),
    process(11, "worker", 30, 10),
    process(12, "other", 10, 20),
  ];
  try {
    const calls: ({ path: string; options: RequestInit } & ReturnType<
        typeof deferred
      >)[] = [],
      errors: unknown[] = [],
      changes: ProcessSummary[][] = [];
    const list = new ListDataStore(
      {
        changed: () => changes.push(list.processes),
        error: (e) => errors.push(e),
      },
      mockApi(async (path, options) => {
        const request = deferred();
        calls.push({ path, options, ...request });
        return request.promise;
      }),
    );
    assert.equal(calls.length, 0, "constructing a page starts no requests");
    const staleStart = list.start();
    list.stop();
    calls[0].resolve({ interval_ms: 5 });
    await staleStart;
    assert.equal(calls.length, 1, "stopped configuration cannot start polling");
    const start = list.start();
    calls[1].resolve({ interval_ms: 5 });
    await flush();
    const oldRequest = calls[2];
    await list.refresh();
    assert.equal(calls.length, 3, "busy reads are skipped");
    list.stop();
    assert.equal(oldRequest.options.signal!.aborted, true);
    const resumed = list.start();
    calls[3].resolve({ interval_ms: 5 });
    await flush();
    oldRequest.resolve([process(99, "stale", 0, 0)]);
    await start;
    assert.equal(
      changes.length,
      0,
      "a completed cancelled request cannot replace observations",
    );
    await list.refresh();
    assert.equal(
      calls.length,
      5,
      "old completion cannot clear a newer busy request",
    );
    calls[4].resolve(processes);
    await resumed;
    assert.equal(timers.size, 1);
    assert.equal(
      [...timers.values()][0].ms,
      1000,
      "minimum list interval is one second",
    );
    assert.deepEqual(list.processes, processes);
    const polling = [...timers.values()][0].fn();
    calls[5].reject(new Error("read denied"));
    await polling;
    assert.equal(errorMessage(errors.at(-1)), "read denied");
    assert.deepEqual(
      list.processes,
      processes,
      "failed updates retain prior observations",
    );
    const recovery = list.refresh();
    calls[6].resolve(processes.slice(1));
    await recovery;
    assert.equal(errors.at(-1), null);
    list.stop();
    assert.equal(timers.size, 0);

    const sources: TestEventSource[] = [],
      events: [string, ...unknown[]][] = [],
      reads: string[] = [];
    let detailMode = "normal",
      held: ReturnType<typeof deferred> | undefined;
    const environment = {
      process_id: id,
      captured_at: 1,
      entries: [{ name: "MODE", value: "<literal>" }],
      lossy_utf8: false,
    };
    const data = new ProcessDataStore(
      {
        identity: (id) => events.push(["identity", id]),
        reset: () => events.push(["reset"]),
        snapshot: () => events.push(["snapshot"]),
        detail: (kind) => events.push(["detail", kind]),
        loading: (show) => events.push(["loading", show]),
        error: (e) => events.push(["error", e]),
      },
      mockApi(async (path) => {
        reads.push(path);
        if (path === "/api/processes") return processes;
        if (detailMode === "hold") {
          held = deferred();
          return held.promise;
        }
        if (detailMode === "denied") throw new Error("permission denied");
        if (detailMode === "wrong identity")
          return {
            ...environment,
            process_id: { ...id, start_time_ticks: 101 },
          };
        return environment;
      }),
      (url) => {
        const source = new TestEventSource(url);
        sources.push(source);
        return source.asEventSource();
      },
    );
    assert.equal(sources.length, 0);
    await data.start("/process/10", "?start_time_ticks=99");
    assert.equal(sources.length, 0, "PID reuse prevents connection");
    assert.match(errorMessage(events.at(-1)![1]), /PID was reused/);
    await data.start("/process/10", "");
    assert.match(
      sources[0].url,
      /^\/api\/processes\/events\?pid=10&start_time_ticks=100$/,
    );
    const sample = threadSample({ tid: 10, sample_age_ms: 5 });
    const target = targetInfo({
      summary: processes[0],
      observation: processObservation({
        threads: [
          threadObservation({ tid: 10 }),
          threadObservation({ tid: 11 }),
        ],
      }),
      live_samples: [sample],
      exited: false,
    });
    sources[0].emit("observation", {
      ...target,
      summary: {
        ...target.summary,
        identity: { ...id, start_time_ticks: 101 },
      },
    });
    assert.equal(data.target, null, "SSE cannot switch process identity");
    const before = performance.now();
    sources[0].emit("observation", target);
    const after = performance.now();
    assert.equal(data.target!.summary.name, "main");
    assert.ok(
      data.sampleAge(sample, after + 100)! >= 105 &&
        data.sampleAge(sample, after + 100)! <= 105 + after - before,
    );
    assert.equal(data.sampleAge(undefined), null);
    const selection = new ThreadSelection();
    selection.retain(data.target);
    assert.equal(selection.tid, 10);
    selection.select(11);
    selection.retain(data.target);
    assert.equal(selection.tid, 11);
    selection.retain({
      ...target,
      observation: processObservation({
        threads: [threadObservation({ tid: 10 })],
      }),
      live_samples: [threadSample({ tid: 11 })],
    });
    assert.equal(
      selection.tid,
      11,
      "last sample preserves selection after a thread exits",
    );
    selection.retain({
      ...target,
      observation: processObservation({
        threads: [threadObservation({ tid: 10 })],
      }),
    });
    assert.equal(selection.tid, 10);

    assert.equal(
      reads.filter((path) => path.includes("/environment?")).length,
      0,
      "closed panels are not read",
    );
    data.setPanelOpen("environment", true);
    await flush();
    assert.deepEqual(data.details.environment.data, environment);
    assert.equal([...timers.values()][0].ms, 5000);
    detailMode = "denied";
    await [...timers.values()][0].fn();
    await flush();
    assert.deepEqual(data.details.environment.data, environment);
    assert.match(
      data.details.environment.error!,
      /permission denied.*Showing the previous result/,
    );
    detailMode = "wrong identity";
    await data.loadDetails("environment");
    assert.deepEqual(
      data.details.environment.data,
      environment,
      "detail response identity is checked",
    );
    detailMode = "hold";
    void data.loadDetails("environment");
    await flush();
    const heldReads = reads.length;
    await data.loadDetails("environment");
    assert.equal(reads.length, heldReads);
    data.stop();
    assert.equal(timers.size, 0);
    assert.equal(sources[0].readyState, 2);
    held!.resolve({ ...environment, entries: [] });
    await flush();
    assert.deepEqual(
      data.details.environment.data,
      environment,
      "pagehide invalidates pending panel response",
    );
    const stoppedEvents = events.length;
    sources[0].emit("observation", target);
    sources[0].error();
    assert.equal(events.length, stoppedEvents);
    const lookups = reads.filter((path) => path === "/api/processes").length;
    data.resume("/process/10", "");
    assert.equal(sources.length, 2);
    assert.equal(
      reads.filter((path) => path === "/api/processes").length,
      lookups,
      "resume reconnects the pinned identity",
    );
    assert.equal(
      data.details.environment.data,
      null,
      "resume resets additional panels",
    );
    sources[1].error();
    assert.match(errorMessage(events.at(-1)![1]), /same process identity/);
    sources[1].emit("observation", target);
    assert.ok(
      events.some(([kind, value]) => kind === "error" && value === null),
    );
    detailMode = "normal";
    data.setPanelOpen("environment", true);
    await flush();
    data.setPanelOpen("environment", false);
    assert.equal(timers.size, 0, "closing all panels stops polling");
    data.setPanelOpen("environment", true);
    await flush();
    sources[1].emit("observation", { ...target, exited: true });
    assert.equal(timers.size, 0);
    assert.equal(sources[1].readyState, 2);
    data.resume("/process/10", "");
    assert.equal(sources.length, 2, "exited processes do not reconnect");
    data.stop();

    const startup = deferred(),
      lateSources: string[] = [];
    const stopped = new ProcessDataStore(
      undefined,
      mockApi(() => startup.promise),
      (url) => {
        lateSources.push(url);
        return new TestEventSource(url).asEventSource();
      },
    );
    const starting = stopped.start("/process/10", "");
    stopped.stop();
    startup.resolve(processes);
    await starting;
    assert.deepEqual(
      lateSources,
      [],
      "old identity lookup cannot open SSE after pagehide",
    );
    assert.equal(
      matchingEnvironment(environment.entries, "MODE=<LITERAL>").length,
      1,
    );
    const descriptors: FileDescriptors["entries"] = [
      {
        fd: 3,
        inode: "1",
        access: "read",
        state: null,
        path: null,
        peer_inode: null,
        note: "",
        kind: "pipe",
        protocol: null,
        local: null,
        remote: null,
        target: "pipe:1",
        peers: [
          {
            process_id: id,
            name: "reader",
            fd: 4,
            access: "write",
            relation: "peer",
          },
        ],
        holders: [],
      },
    ];
    assert.equal(matchingDescriptors(descriptors, "READER").length, 1);
    console.log(
      "List/process data passed: filtering, sorting, cancellation, polling, identity resolution, stale SSE/GET responses, thread selection, additional panels, recovery, and exit.",
    );
  } finally {
    vi.unstubAllGlobals();
  }
});
