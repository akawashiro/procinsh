import { test } from "vitest";
import assert from "node:assert/strict";
import { ListDataStore } from "../src/list/data.ts";
import { matchingProcesses } from "../src/list/search.ts";
import { ProcessDataStore, processRequest } from "../src/process/data.ts";
import { ThreadSelection } from "../src/process/selection.ts";
import {
  matchingEnvironment,
  matchingDescriptors,
} from "../src/process/search.ts";

test("page-data regression", async () => {
  // Exercise page state without a browser or live server.

  const deferred = () => {
    let resolve, reject;
    const promise = new Promise((yes, no) => {
      resolve = yes;
      reject = no;
    });
    return { promise, resolve, reject };
  };
  const flush = () => new Promise((resolve) => setImmediate(resolve));
  const timers = new Map();
  const nativeSetInterval = globalThis.setInterval,
    nativeClearInterval = globalThis.clearInterval;
  let timerId = 0;
  globalThis.setInterval = (fn, ms) => {
    const id = ++timerId;
    timers.set(id, { fn, ms });
    return id;
  };
  globalThis.clearInterval = (id) => timers.delete(id);
  const id = { pid: 10, start_time_ticks: 100 };
  const process = (pid, name, cpu_percent, rss_bytes) => ({
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
    const calls = [],
      errors = [],
      changes = [];
    const list = new ListDataStore(
      {
        changed: () => changes.push(list.processes),
        error: (e) => errors.push(e),
      },
      (path, options) => {
        const request = deferred();
        calls.push({ path, options, ...request });
        return request.promise;
      },
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
    assert.equal(oldRequest.options.signal.aborted, true);
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
    assert.equal(errors.at(-1).message, "read denied");
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

    assert.deepEqual(
      matchingProcesses(processes, " --WORKER ", "pid").map(
        (p) => p.identity.pid,
      ),
      [10, 11, 12],
    );
    assert.deepEqual(
      matchingProcesses(processes, "WORK", "rss").map((p) => p.identity.pid),
      [12, 11, 10],
    );
    assert.deepEqual(
      matchingProcesses(processes, "", "cpu").map((p) => p.identity.pid),
      [11, 12, 10],
    );
    assert.deepEqual(
      processes.map((p) => p.identity.pid),
      [10, 11, 12],
      "sorting does not reorder stored data",
    );

    class Source extends EventTarget {
      readyState = 0;
      constructor(url) {
        super();
        this.url = url;
      }
      close() {
        this.readyState = 2;
      }
      emit(target) {
        this.dispatchEvent(
          new MessageEvent("observation", { data: JSON.stringify(target) }),
        );
      }
      error() {
        this.onerror?.(new Event("error"));
      }
    }
    const sources = [],
      events = [],
      reads = [];
    let detailMode = "normal",
      held;
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
      async (path) => {
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
      },
      (url) => {
        const source = new Source(url);
        sources.push(source);
        return source;
      },
    );
    assert.equal(sources.length, 0);
    await data.start("/process/10", "?start_time_ticks=99");
    assert.equal(sources.length, 0, "PID reuse prevents connection");
    assert.match(events.at(-1)[1].message, /PID was reused/);
    await data.start("/process/10", "");
    assert.match(
      sources[0].url,
      /^\/api\/processes\/events\?pid=10&start_time_ticks=100$/,
    );
    const sample = { tid: 10, sample_age_ms: 5 };
    const target = {
      summary: processes[0],
      observation: { threads: [{ tid: 10 }, { tid: 11 }] },
      live_samples: [sample],
      exited: false,
    };
    sources[0].emit({
      ...target,
      summary: {
        ...target.summary,
        identity: { ...id, start_time_ticks: 101 },
      },
    });
    assert.equal(data.target, null, "SSE cannot switch process identity");
    const before = performance.now();
    sources[0].emit(target);
    const after = performance.now();
    assert.equal(data.target.summary.name, "main");
    assert.ok(
      data.sampleAge(sample, after + 100) >= 105 &&
        data.sampleAge(sample, after + 100) <= 105 + after - before,
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
      observation: { threads: [{ tid: 10 }] },
      live_samples: [{ tid: 11 }],
    });
    assert.equal(
      selection.tid,
      11,
      "last sample preserves selection after a thread exits",
    );
    selection.retain({ ...target, observation: { threads: [{ tid: 10 }] } });
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
      data.details.environment.error,
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
    held.resolve({ ...environment, entries: [] });
    await flush();
    assert.deepEqual(
      data.details.environment.data,
      environment,
      "pagehide invalidates pending panel response",
    );
    const stoppedEvents = events.length;
    sources[0].emit(target);
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
    assert.match(events.at(-1)[1].message, /same process identity/);
    sources[1].emit(target);
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
    sources[1].emit({ ...target, exited: true });
    assert.equal(timers.size, 0);
    assert.equal(sources[1].readyState, 2);
    data.resume("/process/10", "");
    assert.equal(sources.length, 2, "exited processes do not reconnect");
    data.stop();

    const startup = deferred(),
      lateSources = [];
    const stopped = new ProcessDataStore(
      undefined,
      () => startup.promise,
      (url) => {
        lateSources.push(url);
        return new Source(url);
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
    assert.deepEqual(processRequest("/process/10", "?start_time_ticks=100"), {
      pid: 10,
      startTime: 100,
    });
    for (const [path, search] of [
      ["/process/0", ""],
      ["/list", ""],
      ["/process/9007199254740992", ""],
      ["/process/10", "?start_time_ticks=-1"],
      ["/process/10", "?start_time_ticks=1&start_time_ticks=2"],
      ["/process/10", "?start_time_ticks=9007199254740992"],
    ])
      assert.throws(() => processRequest(path, search));
    assert.equal(
      matchingEnvironment(environment.entries, "MODE=<LITERAL>").length,
      1,
    );
    const descriptors = [
      {
        fd: 3,
        kind: "pipe",
        protocol: null,
        local: null,
        remote: null,
        target: "pipe:1",
        peers: [{ process_id: id, name: "reader" }],
        holders: [],
      },
    ];
    assert.equal(matchingDescriptors(descriptors, "READER").length, 1);
    console.log(
      "List/process data passed: filtering, sorting, cancellation, polling, identity resolution, stale SSE/GET responses, thread selection, additional panels, recovery, and exit.",
    );
  } finally {
    globalThis.setInterval = nativeSetInterval;
    globalThis.clearInterval = nativeClearInterval;
  }
});
