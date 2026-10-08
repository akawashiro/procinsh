// Exercise Vite's browser routes, CSS HMR, and JSON/SSE proxy with a real backend.
import assert from "node:assert/strict";
import {
  launch as spawnCaptured,
  repositoryPath,
  type TestProcess,
} from "../support/runtime.js";
import type { SpawnOptions } from "node:child_process";
import type { ProcessSummary } from "../../src/shared/api-types.js";
import { readFile, writeFile } from "node:fs/promises";

const children: TestProcess[] = [];
const stylesheet = new URL("../../src/list/style.css", import.meta.url);
let originalStylesheet: string | undefined, hmrSocket: WebSocket | undefined;
function launch(command: string, args: string[], options: SpawnOptions = {}) {
  const child = spawnCaptured(command, args, options);
  children.push(child);
  return child;
}
async function address(child: TestProcess) {
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) {
    const url = child.output.match(/http:\/\/127\.0\.0\.1:\d+/)?.[0];
    if (url) return url;
    if (child.exitCode !== null) throw new Error(child.output);
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`Startup timed out: ${child.output}`);
}
try {
  const backend = launch("./scripts/dev_run.sh", ["--listen", "127.0.0.1:0"]);
  const backendUrl = await address(backend);
  const vite = launch(
    process.execPath,
    [
      repositoryPath("web/node_modules/vite/bin/vite.js"),
      "--host",
      "127.0.0.1",
      "--port",
      "0",
    ],
    {
      cwd: repositoryPath("web"),
      env: { ...process.env, PROCINSH_BACKEND_URL: backendUrl },
    },
  );
  const url = await address(vite);
  const headers = { origin: url, "sec-fetch-site": "same-origin" };
  for (const path of [
    "/",
    "/list",
    "/space",
    "/process/123?start_time_ticks=456",
  ]) {
    const response = await fetch(url + path);
    assert.equal(response.status, 200, path);
    const html = await response.text();
    assert.ok(html.includes("/@vite/client"));
    assert.ok(html.includes(" · development"));
    assert.ok(!html.includes("{{PROCINSH_"));
    assert.ok(
      html.includes(
        path.startsWith("/process")
          ? "/src/process/app.ts"
          : path === "/space"
            ? "/src/space/app.ts"
            : "/src/list/app.ts",
      ),
    );
  }
  const client = await (await fetch(url + "/@vite/client")).text();
  const token = client.match(/const wsToken = "([^"]+)"/)![1];
  hmrSocket = new WebSocket(
    url.replace("http:", "ws:") + "/?token=" + token,
    "vite-hmr",
  );
  await new Promise<void>((resolve, reject) => {
    hmrSocket!.addEventListener("open", () => resolve(), { once: true });
    hmrSocket!.addEventListener("error", reject, { once: true });
  });
  await fetch(url + "/src/list/style.css");
  originalStylesheet = await readFile(stylesheet, "utf8");
  const updated = new Promise<void>((resolve, reject) => {
    const timeout = setTimeout(
      () => reject(new Error("CSS HMR update timed out")),
      10000,
    );
    hmrSocket!.addEventListener("message", (event) => {
      const message = JSON.parse(event.data);
      if (
        message.type === "update" &&
        message.updates.some((update: { path: string }) =>
          update.path.includes("/src/list/style.css"),
        )
      ) {
        clearTimeout(timeout);
        resolve();
      }
    });
  });
  await writeFile(
    stylesheet,
    originalStylesheet + "\n/* HMR regression fixture */\n",
  );
  await updated;
  assert.equal((await fetch(url + "/src/space/app.ts")).status, 200);
  const direct = await (await fetch(backendUrl + "/api/config")).json();
  assert.deepEqual(
    await (await fetch(url + "/api/config", { headers })).json(),
    direct,
  );
  assert.equal(
    (
      await fetch(url + "/api/config", {
        headers: { ...headers, origin: "http://evil.test" },
      })
    ).status,
    403,
  );

  const sleeper = launch("sleep", ["30"]);
  const processes: ProcessSummary[] = await (
    await fetch(url + "/api/processes", { headers })
  ).json();
  const identity = processes.find(
    (p) => p.identity.pid === sleeper.pid,
  )!.identity;
  for (const [path, event] of [
    ["/api/system/events", "snapshot"],
    [
      `/api/processes/events?pid=${identity.pid}&start_time_ticks=${identity.start_time_ticks}`,
      "observation",
    ],
  ]) {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 10000);
    try {
      const response = await fetch(url + path, {
        headers,
        signal: controller.signal,
      });
      assert.equal(response.status, 200);
      assert.ok(
        response.headers.get("content-type")!.startsWith("text/event-stream"),
      );
      const reader = response.body!.getReader(),
        decoder = new TextDecoder();
      let text = "";
      while (!text.includes(`event: ${event}\n`) || !text.includes("\n\n")) {
        const chunk = await reader.read();
        assert.ok(!chunk.done, "SSE stays open");
        text += decoder.decode(chunk.value, { stream: true });
      }
      assert.ok(text.includes("data: "));
    } finally {
      clearTimeout(timeout);
      controller.abort();
    }
  }
  console.log(
    "Vite development routes, CSS HMR, origin guard, JSON and both SSE proxies passed.",
  );
} finally {
  if (originalStylesheet !== undefined)
    await writeFile(stylesheet, originalStylesheet);
  hmrSocket?.close();
  await Promise.all(
    children.reverse().map(
      (child) =>
        new Promise<void>((resolve) => {
          if (child.exitCode !== null) return resolve();
          child.once("exit", () => resolve());
          child.kill("SIGTERM");
        }),
    ),
  );
}
