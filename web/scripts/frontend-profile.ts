// Collect standard Chrome DevTools artifacts without instrumenting the application.
import { parseArgs } from "node:util";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir, cpus, release } from "node:os";
import { join, resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { SourceMap } from "node:module";
import { gzipSync } from "node:zlib";
import { connectPage, pages } from "../tests/browser/harness.js";
import { launch, delay, until } from "../tests/support/runtime.js";
import { profileFixture } from "./frontend-profile-fixture.js";

const { values } = parseArgs({
  options: {
    url: { type: "string", default: "http://127.0.0.1:9110" },
    output: { type: "string", default: "../target/frontend-profile" },
    scenario: { type: "string", default: "idle-space" },
    seconds: { type: "string", default: "20" },
    fixture: { type: "boolean", default: false },
    swiftshader: { type: "boolean", default: false },
  },
});
const scenarios = ["idle-space", "interact-space", "busy-space", "idle-list"];
if (!scenarios.includes(values.scenario))
  throw new Error(`--scenario must be ${scenarios.join(", ")}`);
const seconds = Number(values.seconds);
if (!Number.isFinite(seconds) || seconds < 15 || seconds > 30)
  throw new Error("--seconds must be between 15 and 30");
const output = resolve(values.output);
await mkdir(output, { recursive: true });
const directory = await mkdtemp(join(tmpdir(), "procinsh-profile-"));
const fixture = values.fixture
  ? await profileFixture(values.url, values.scenario === "busy-space")
  : undefined;
const chromeArgs = [
  "--headless=new",
  "--no-sandbox",
  "--no-first-run",
  "--disable-extensions",
  "--disable-dev-shm-usage",
  "--use-gl=angle",
  ...(values.swiftshader
    ? ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"]
    : ["--use-angle=gl-egl"]),
  "--remote-debugging-port=0",
  `--user-data-dir=${directory}`,
  "about:blank",
];
const chrome = launch(
  process.env.CHROME || "/usr/bin/google-chrome",
  chromeArgs,
);
let page: Awaited<ReturnType<typeof connectPage>> | undefined;
let browser: Awaited<ReturnType<typeof connectPage>> | undefined;
const errors: string[] = [];
const timeout = setTimeout(() => {
  console.error("Profiling timed out");
  chrome.kill("SIGKILL");
}, 180_000);
try {
  const port = await until(
    () =>
      chrome.output.match(
        /ws:\/\/127\.0\.0\.1:(\d+)\/devtools\/browser\/[\w-]+/,
      )?.[1],
    "Chrome DevTools",
  );
  const version = await (
    await fetch(`http://127.0.0.1:${port}/json/version`)
  ).json();
  browser = await connectPage(version.webSocketDebuggerUrl);
  const gpu = await browser.cdp("SystemInfo.getInfo");
  page = await connectPage(
    (await pages(port)).find((p) => p.type === "page")!.webSocketDebuggerUrl,
    errors,
  );
  const { cdp, evaluate, waitFor, socket } = page;
  await cdp("Page.enable");
  await cdp("Runtime.enable");
  await cdp("Log.enable");
  await cdp("Network.enable");
  await cdp("Performance.enable");
  let counts: unknown = fixture?.counts;
  socket.addEventListener("message", (event: MessageEvent<string>) => {
    const message = JSON.parse(event.data);
    if (
      message.method === "Network.eventSourceMessageReceived" &&
      message.params.eventName === "snapshot"
    ) {
      const snapshot = JSON.parse(message.params.data) as {
        processes: { maps?: unknown[] }[];
        fd_relations?: unknown[];
      };
      if (!fixture)
        counts = {
          processes: snapshot.processes.length,
          mapsIncluded: snapshot.processes.reduce(
            (n, p) => n + (p.maps?.length ?? 0),
            0,
          ),
          relationsIncluded: snapshot.fd_relations?.length,
        };
    }
  });
  await cdp("Emulation.setDeviceMetricsOverride", {
    width: 1440,
    height: 1100,
    deviceScaleFactor: 1,
    mobile: false,
  });
  const url =
    (fixture?.url ?? values.url) +
    (values.scenario === "idle-list" ? "/list" : "/space");
  await cdp("Page.navigate", { url });
  await waitFor(
    values.scenario === "idle-list"
      ? "document.querySelectorAll('#process-list tr').length>2"
      : "document.getElementById('fps')?.textContent.includes('FPS')",
    "rendered page",
  );
  await delay(5000);
  const state = () =>
    evaluate(
      "({hidden:document.hidden,width:innerWidth,height:innerHeight,dpr:devicePixelRatio,fps:document.getElementById('fps')?.textContent,heap:performance.memory?.usedJSHeapSize,buildRevision:document.getElementById('build-commit')?.href,buildLabel:document.getElementById('build-commit')?.textContent})",
    );
  const beforeState = await state();
  const beforeMetrics = await cdp("Performance.getMetrics");
  const beforeProcesses = await browser.cdp("SystemInfo.getProcessInfo");
  await cdp("Profiler.enable");
  await cdp("Profiler.start");
  await cdp("Tracing.start", {
    categories: "devtools.timeline,v8,disabled-by-default-devtools.timeline",
    transferMode: "ReturnAsStream",
  });
  const started = performance.now();
  if (values.scenario === "interact-space") {
    let i = 0;
    while (performance.now() - started < seconds * 1000) {
      const button = Math.floor(i / 120) % 2 ? "right" : "left";
      if (i % 120 === 0)
        await cdp("Input.dispatchMouseEvent", {
          type: "mousePressed",
          x: 720,
          y: 600,
          button,
          buttons: button === "left" ? 1 : 2,
          clickCount: 1,
        });
      await cdp("Input.dispatchMouseEvent", {
        type: "mouseMoved",
        x: 720 + 180 * Math.sin(i / 20),
        y: 600 + 80 * Math.sin(i / 30),
        button,
        buttons: button === "left" ? 1 : 2,
      });
      if (i % 120 === 119) {
        await cdp("Input.dispatchMouseEvent", {
          type: "mouseReleased",
          x: 720,
          y: 600,
          button,
          buttons: 0,
          clickCount: 1,
        });
        await cdp("Input.dispatchMouseEvent", {
          type: "mouseWheel",
          x: 720,
          y: 600,
          deltaX: 0,
          deltaY: Math.floor(i / 120) % 2 ? 80 : -80,
        });
      }
      i++;
      await delay(16);
    }
    await cdp("Input.dispatchMouseEvent", {
      type: "mouseReleased",
      x: 720,
      y: 600,
      button: "left",
      buttons: 0,
      clickCount: 1,
    });
    await cdp("Input.dispatchMouseEvent", {
      type: "mouseReleased",
      x: 720,
      y: 600,
      button: "right",
      buttons: 0,
      clickCount: 1,
    });
  } else await delay(seconds * 1000);
  const elapsedSeconds = (performance.now() - started) / 1000;
  const { profile } = await cdp("Profiler.stop");
  const afterMetrics = await cdp("Performance.getMetrics");
  const afterProcesses = await browser.cdp("SystemInfo.getProcessInfo");
  const afterState = await state();
  const complete = new Promise<string>((resolve, reject) => {
    const closed = () =>
      reject(new Error("Chrome closed before the trace completed"));
    const listener = (event: MessageEvent<string>) => {
      const message = JSON.parse(event.data);
      if (message.method === "Tracing.tracingComplete") {
        socket.removeEventListener("message", listener);
        socket.removeEventListener("close", closed);
        resolve(message.params.stream);
      }
    };
    socket.addEventListener("message", listener);
    socket.addEventListener("close", closed, { once: true });
  });
  await cdp("Tracing.end");
  const handle = await complete;
  let trace = "";
  try {
    while (true) {
      const chunk = await cdp("IO.read", { handle });
      trace += chunk.base64Encoded
        ? Buffer.from(chunk.data, "base64").toString()
        : chunk.data;
      if (chunk.eof) break;
    }
  } finally {
    await cdp("IO.close", { handle });
  }
  const prefix = join(output, values.scenario);
  await writeFile(`${prefix}.cpuprofile.gz`, gzipSync(JSON.stringify(profile)));
  await writeFile(`${prefix}.trace.json.gz`, gzipSync(trace));
  const maps = new Map<string, SourceMap>();
  for (const url of new Set(
    profile.nodes
      .map((n) => n.callFrame.url)
      .filter((url) => /\/assets\/.*\.js$/.test(url)),
  )) {
    const response = await fetch(`${url}.map`);
    if (response.ok) {
      const payload = await response.json();
      maps.set(url, new SourceMap(payload));
      await writeFile(
        join(output, new URL(url).pathname.split("/").pop()! + ".map.gz"),
        gzipSync(JSON.stringify(payload)),
      );
    }
  }
  const nodes = new Map(profile.nodes.map((node) => [node.id, node]));
  const self = new Map<number, number>();
  profile.samples?.forEach((id, i) =>
    self.set(id, (self.get(id) ?? 0) + (profile.timeDeltas?.[i] ?? 0)),
  );
  const topSelf = [...self]
    .sort((a, b) => b[1] - a[1])
    .slice(0, 30)
    .map(([id, us]) => {
      const frame = nodes.get(id)!.callFrame;
      return {
        milliseconds: us / 1000,
        frame,
        source: maps
          .get(frame.url)
          ?.findEntry(frame.lineNumber, frame.columnNumber),
      };
    });
  type TraceEvent = {
    name: string;
    ph: string;
    pid: number;
    tid: number;
    dur?: number;
    args?: { name?: string };
  };
  const events = JSON.parse(trace).traceEvents as TraceEvent[];
  const main = events.find(
    (e) => e.name === "thread_name" && e.args?.name === "CrRendererMain",
  );
  const durations = (name: string) =>
    events
      .filter(
        (e) =>
          e.ph === "X" &&
          e.name === name &&
          e.pid === main?.pid &&
          e.tid === main.tid,
      )
      .map((e) => (e.dur ?? 0) / 1000)
      .sort((a, b) => a - b);
  const distribution = (times: number[]) => ({
    count: times.length,
    totalMs: times.reduce((a, b) => a + b, 0),
    p50Ms: times[Math.floor(times.length * 0.5)] ?? 0,
    p95Ms: times[Math.floor(times.length * 0.95)] ?? 0,
    maxMs: times.at(-1) ?? 0,
  });
  const metric = (name: string) =>
    afterMetrics.metrics.find((m) => m.name === name)!.value -
    beforeMetrics.metrics.find((m) => m.name === name)!.value;
  const processCpu: Record<string, number> = {};
  for (const p of afterProcesses.processInfo) {
    const previous = beforeProcesses.processInfo.find((old) => old.id === p.id);
    if (previous)
      processCpu[p.type] =
        (processCpu[p.type] ?? 0) +
        ((p.cpuTime - previous.cpuTime) / elapsedSeconds) * 100;
  }
  const metadata = {
    scenario: values.scenario,
    fixture: values.fixture,
    url,
    releaseBackend: values.url,
    collectedAt: new Date().toISOString(),
    commit: execFileSync("git", ["rev-parse", "HEAD"], {
      encoding: "utf8",
    }).trim(),
    dirty: execFileSync("git", ["status", "--porcelain"], {
      encoding: "utf8",
    }).trim(),
    hardware: {
      cpu: cpus()[0].model,
      logicalCpus: cpus().length,
      kernel: release(),
    },
    version,
    gpu: gpu.gpu,
    chromeArgs,
    counts,
    elapsedSeconds,
    beforeState,
    afterState,
    processCpuPercentOfOneCore: processCpu,
    taskBusyPercent: (metric("TaskDuration") / elapsedSeconds) * 100,
    scriptSeconds: metric("ScriptDuration"),
    layoutSeconds: metric("LayoutDuration"),
    animationFrames: distribution(durations("FireAnimationFrame")),
    longTasks: distribution(durations("RunTask").filter((ms) => ms > 50)),
    gc: distribution(
      [...durations("MinorGC"), ...durations("MajorGC")].sort((a, b) => a - b),
    ),
    layout: distribution(durations("Layout")),
    paint: distribution(durations("Paint")),
    beforeMetrics,
    afterMetrics,
    beforeProcesses,
    afterProcesses,
    topSelf,
    errors,
  };
  await writeFile(`${prefix}.json`, JSON.stringify(metadata, null, 2) + "\n");
  if (await evaluate<boolean>("document.hidden"))
    throw new Error("Profiling page must stay visible");
  if (errors.length) throw new Error(`Browser errors: ${errors.join("\n")}`);
  console.log(
    JSON.stringify({
      prefix,
      taskBusyPercent: metadata.taskBusyPercent,
      processCpu,
      animationFrames: metadata.animationFrames,
      longTasks: metadata.longTasks,
    }),
  );
} finally {
  clearTimeout(timeout);
  page?.socket.close();
  browser?.socket.close();
  chrome.kill("SIGTERM");
  await delay(300);
  if (chrome.exitCode === null && chrome.signalCode === null)
    chrome.kill("SIGKILL");
  await fixture?.close();
  await rm(directory, { recursive: true, force: true });
}
