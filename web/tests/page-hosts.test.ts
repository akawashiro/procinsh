import { test, vi } from "vitest";

import assert from "node:assert/strict";
import { ListDataStore } from "../src/list/data.js";
import { ProcessDataStore } from "../src/process/data.js";
import { hostUrl, readHostParam, withHost } from "../src/shared/hosts.js";
import { processUrl, query } from "../src/shared/navigation.js";
import { processSummary, targetInfo } from "./support/fixtures.js";
import { mockApi, TestEventSource } from "./support/mocks.js";

const flush = () => new Promise((resolve) => setImmediate(resolve));

test("host parameters and links name only remote hosts", () => {
  assert.equal(readHostParam("http://x/list"), null);
  assert.equal(readHostParam("http://x/list?host="), null);
  assert.equal(readHostParam("http://x/list?host=node-b"), "node-b");
  assert.equal(
    hostUrl("node-b", "http://x/list?a=1"),
    "http://x/list?a=1&host=node-b",
  );
  assert.equal(hostUrl(null, "http://x/list?host=node-b"), "http://x/list");
  assert.equal(withHost("/api/processes", null), "/api/processes");
  assert.equal(withHost("/api/processes", "a b"), "/api/processes?host=a+b");
  assert.equal(withHost("/list?x=1", "b"), "/list?x=1&host=b");
  const id = { pid: 7, start_time_ticks: 9 };
  assert.equal(processUrl(id), "/process/7?start_time_ticks=9");
  assert.equal(processUrl(id, "b"), "/process/7?start_time_ticks=9&host=b");
  assert.equal(
    processUrl({ ...id, host: "c" }, "b"),
    "/process/7?start_time_ticks=9&host=c",
    "a host-qualified identity names its own host",
  );
  assert.equal(query(id), "pid=7&start_time_ticks=9");
  assert.equal(query(id, "b"), "pid=7&start_time_ticks=9&host=b");
});

test("the list reads the selected host and drops the previous host's rows", async () => {
  vi.stubGlobal("setInterval", () => 1);
  vi.stubGlobal("clearInterval", () => {});
  try {
    const paths: string[] = [];
    const list = new ListDataStore(
      undefined,
      mockApi(async (path) => {
        paths.push(path);
        if (path === "/api/config") return { interval_ms: 1000 };
        return [processSummary({ name: path })];
      }),
    );
    await list.start();
    assert.equal(list.processes[0].name, "/api/processes");
    list.setHost("node-b");
    assert.equal(list.processes.length, 0);
    await flush();
    assert.equal(list.processes[0].name, "/api/processes?host=node-b");
    list.setHost(null);
    await flush();
    assert.deepEqual(paths, [
      "/api/config",
      "/api/processes",
      "/api/config",
      "/api/processes?host=node-b",
      "/api/config",
      "/api/processes",
    ]);
    list.stop();
  } finally {
    vi.unstubAllGlobals();
  }
});

test("a remote process page resolves, observes, and loads details on its host", async () => {
  const id = { pid: 10, start_time_ticks: 100 };
  const paths: string[] = [];
  const sources: TestEventSource[] = [];
  const store = new ProcessDataStore(
    undefined,
    mockApi(async (path) => {
      paths.push(path);
      if (path.startsWith("/api/processes?"))
        return [processSummary({ identity: id })];
      return { process_id: id, entries: [], warnings: [] };
    }),
    (url) => {
      const source = new TestEventSource(url);
      sources.push(source);
      return source.asEventSource();
    },
  );
  await store.start("/process/10", "?start_time_ticks=100&host=node-b");
  assert.equal(store.host, "node-b");
  assert.equal(
    sources[0].url,
    "/api/processes/events?pid=10&start_time_ticks=100&host=node-b",
  );
  sources[0].emit(
    "observation",
    targetInfo({ summary: processSummary({ identity: id }) }),
  );
  store.setPanelOpen("fds", true);
  await flush();
  assert.deepEqual(paths, [
    "/api/processes?host=node-b",
    "/api/processes/fds?pid=10&start_time_ticks=100&host=node-b",
  ]);
  store.stop();
});
