// Use the existing browser recorder through QEMU hostfwd; no alternate JSON writer.
import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { gzipSync } from "node:zlib";
import { connectPage, pages } from "../tests/browser/harness.js";
import { launch, until, delay } from "../tests/support/runtime.js";
import { parseRecording, MAX_RECORDING_BYTES } from "../src/space/recording.js";
import type { ProcessId } from "../src/shared/api-types.js";
import { PAGES_MAX_ASSET_BYTES } from "../vite.replay.config.js";

const workload = process.env.WORKLOAD ?? "chromium";
assert.ok(["chromium", "firefox", "kernel-build"].includes(workload));
const kernelBuild = workload === "kernel-build";
const workloadName = kernelBuild
  ? "Linux kernel build"
  : workload === "firefox"
    ? "Firefox"
    : "Chromium";
function isWorkloadProcess(process: {
  name: string;
  command_line?: string[] | null;
}) {
  if (kernelBuild) {
    const executable =
      process.command_line?.[0]?.split("/").pop() ?? process.name;
    return /^clang(?:-\d+)?$/.test(executable);
  }
  return (
    process.name.toLowerCase().includes(workload) ||
    !!process.command_line?.[0]?.toLowerCase().includes(workload)
  );
}
function processKey(identity: ProcessId) {
  return `${identity.pid}:${identity.start_time_ticks}`;
}
// Keep collection active until a compiler survives long enough for a CPU delta.
async function waitForCompilerCpu(base: string) {
  const response = await fetch(`${base}/api/system/events`, {
    signal: AbortSignal.timeout(300000),
  });
  assert.ok(response.ok && response.body, "VM activity stream required");
  const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
  const compilers = new Set<string>();
  let pending = "";
  try {
    while (true) {
      const { done, value } = await reader.read();
      assert.ok(!done, "Activity stream ended before compiler CPU activity");
      pending += value;
      let newline: number;
      while ((newline = pending.indexOf("\n")) >= 0) {
        const line = pending.slice(0, newline).trim();
        pending = pending.slice(newline + 1);
        if (!line.startsWith("data: ")) continue;
        const event = JSON.parse(line.slice(6));
        for (const process of event.processes ?? []) {
          if (isWorkloadProcess(process))
            compilers.add(processKey(process.identity));
        }
        if (
          event.cpu?.some(
            (sample: { process_id: ProcessId; runtime_ns: number }) =>
              compilers.has(processKey(sample.process_id)) &&
              sample.runtime_ns > 0,
          )
        )
          return;
      }
    }
  } finally {
    await reader.cancel();
  }
}
const port = Number(process.env.HOST_PORT ?? 10090);
const seconds = Number(process.env.RECORD_SECONDS ?? (kernelBuild ? 30 : 15));
assert.ok(Number.isInteger(port) && port > 0 && port <= 65535);
assert.ok(Number.isFinite(seconds) && seconds >= 1 && seconds <= 300);
const output = process.env.RECORD_OUTPUT ?? `/tmp/${workload}-vm.json.gz`;
const base = `http://127.0.0.1:${port}`;
const profile = await mkdtemp(join(tmpdir(), "space-vm-capture-"));
const errors: string[] = [];
const chromium = launch(process.env.CHROMIUM || "/usr/bin/chromium", [
  "--headless=new",
  "--use-gl=angle",
  "--use-angle=swiftshader",
  "--enable-unsafe-swiftshader",
  "--disable-dev-shm-usage",
  "--remote-debugging-port=0",
  ...(process.getuid?.() === 0 ? ["--no-sandbox"] : []),
  `--user-data-dir=${profile}`,
  "about:blank",
]);
let socket: WebSocket | undefined;
try {
  const endpoint = await until(
    () =>
      chromium.output.match(
        /ws:\/\/127\.0\.0\.1:\d+\/devtools\/browser\/[\w-]+/,
      )?.[0],
    "Chromium debugger",
  );
  const page = (await pages(new URL(endpoint).port)).find(
    (p) => p.type === "page",
  )!;
  const connection = await connectPage(page.webSocketDebuggerUrl, errors);
  socket = connection.socket;
  const { cdp, evaluate, waitFor } = connection;
  await cdp("Runtime.enable");
  await cdp("Page.enable");
  await until(
    async () => {
      const response = await fetch(`${base}/api/processes`);
      assert.ok(
        response.ok,
        `VM process discovery failed (${response.status})`,
      );
      const processes = (await response.json()) as {
        name: string;
        command_line?: string[] | null;
      }[];
      return (
        processes.filter(isWorkloadProcess).length >= (kernelBuild ? 1 : 4)
      );
    },
    `${workloadName} workload processes`,
    45000,
  );
  console.log(`Opening ${base}/space`);
  await cdp("Page.navigate", { url: `${base}/space` });
  await waitFor(
    "document.querySelector('#record')?.disabled===false",
    "VM recording baseline",
  );
  // Let the lazy live collector refresh a snapshot retained while it was idle.
  await delay(2000);
  if (kernelBuild) {
    console.log("Waiting for measurable compiler CPU activity");
    await waitForCompilerCpu(base);
  } else {
    // A retained idle snapshot can be empty until the first live refresh arrives.
    await until(
      () =>
        evaluate<boolean>(`(() => {
          const search = document.querySelector('#search');
          search.value = ${JSON.stringify(workload)};
          search.dispatchEvent(new Event('input'));
          search.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter'}));
          return !document.querySelector('#details').hidden &&
            document.querySelector('#name').textContent.toLowerCase().includes(${JSON.stringify(workload)});
        })()`),
      `${workloadName} live snapshot`,
      45000,
    );
  }
  console.log(`Recording ${seconds} seconds of VM activity`);
  await evaluate(`window.recordingBlob=null;
    URL.createObjectURL=blob=>{window.recordingBlob=blob;return 'blob:recording'};
    HTMLAnchorElement.prototype.click=function(){};
    document.querySelector('#record').click()`);
  for (let elapsed = 0; elapsed < seconds; elapsed++) {
    await delay(1000);
    assert.match(
      await evaluate<string>(
        "document.querySelector('#recording-status').textContent",
      ),
      /REC/,
      "Recorder must remain continuous",
    );
  }
  await evaluate("document.querySelector('#download-recording').click()");
  const text = await evaluate<string>("window.recordingBlob.text()");
  assert.ok(Buffer.byteLength(text) <= MAX_RECORDING_BYTES);
  const recording = parseRecording(text);
  assert.ok(
    recording.initial_snapshot.processes.length > 0,
    "Nonempty recording baseline required",
  );
  assert.equal(
    recording.end_reason,
    "Stopped by user",
    "Recording must finish normally",
  );
  assert.ok(recording.duration_ms >= seconds * 1000);
  console.log(
    `Baseline: ${recording.initial_snapshot.processes.length} processes, ${recording.events.length} events`,
  );
  const zipped = gzipSync(text, { level: 9 });
  assert.ok(zipped.length <= PAGES_MAX_ASSET_BYTES);
  const workloadProcesses = new Set(
    [
      recording.initial_snapshot,
      ...recording.events
        .filter((event) => event.type === "snapshot")
        .map((event) => event.data),
    ]
      .flatMap((snapshot) => snapshot.processes)
      .filter(isWorkloadProcess)
      .map((p) => processKey(p.identity)),
  );
  assert.ok(
    workloadProcesses.size >= (kernelBuild ? 1 : 4),
    `Real ${workloadName} process tree required`,
  );
  const activities = recording.events.filter(
    (event) => event.type === "activity",
  );
  assert.ok(activities.length > 0, "Activity events required");
  const observations = {
    cpu: activities
      .flatMap((event) => event.data.cpu ?? [])
      .filter(
        (sample) =>
          workloadProcesses.has(processKey(sample.process_id)) &&
          sample.runtime_ns > 0,
      ).length,
    ipc: activities
      .flatMap((event) => event.data.ipc ?? [])
      .filter(
        (sample) =>
          workloadProcesses.has(processKey(sample.process_id)) &&
          sample.bytes > 0,
      ).length,
    files: activities
      .flatMap((event) => event.data.files ?? [])
      .filter(
        (sample) =>
          workloadProcesses.has(processKey(sample.process_id)) &&
          sample.bytes > 0,
      ).length,
  };
  for (const sensor of kernelBuild
    ? (["cpu", "files"] as const)
    : (["cpu", "ipc", "files"] as const))
    assert.ok(
      observations[sensor] > 0,
      `${workloadName} ${sensor} activity required`,
    );
  await cdp("Page.navigate", { url: `${base}/replay/space/` });
  await waitFor("document.querySelector('#replay-status')", "VM replay page");
  await evaluate(`(() => {
    const bytes=Uint8Array.from(atob(${JSON.stringify(zipped.toString("base64"))}), c=>c.charCodeAt(0));
    const transfer=new DataTransfer();transfer.items.add(new File([bytes],'workload-vm.json.gz'));
    const input=document.querySelector('input[type=file]');input.files=transfer.files;input.dispatchEvent(new Event('change'));
  })()`);
  await waitFor(
    "document.querySelector('#replay-status')?.hidden && document.querySelector('#play').textContent==='Pause'",
    "Real gzip replays",
  );
  assert.deepEqual(
    errors,
    [],
    "Recording and replay must not raise browser errors",
  );
  await writeFile(output, zipped, { flag: "wx" });
  console.log(JSON.stringify(observations));
  console.log(
    `Saved ${output}: ${Buffer.byteLength(text)} JSON bytes, ${zipped.length} gzip bytes, ${workloadProcesses.size} ${workloadName} processes`,
  );
} finally {
  socket?.close();
  chromium.kill("SIGTERM");
  await until(
    () => chromium.exitCode !== null || chromium.signalCode !== null,
    "capture Chromium exit",
  );
  await rm(profile, { recursive: true, force: true });
}
