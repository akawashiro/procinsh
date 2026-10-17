// Serve only replay build output at a deployment prefix; no application backend.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile, mkdtemp, rm } from "node:fs/promises";
import { join, resolve, extname } from "node:path";
import { tmpdir } from "node:os";
import { connectPage, pages } from "./harness.js";
import { launch, until, delay } from "../support/runtime.js";

const root = resolve(import.meta.dirname, "../../dist-replay");
const requests: string[] = [];
const server = createServer(async (request, response) => {
  const url = new URL(request.url!, "http://localhost");
  requests.push(url.pathname);
  const path = resolve(
    root,
    url.pathname.replace(/^\/demo\//, "") +
      (url.pathname.endsWith("/") ? "index.html" : ""),
  );
  if (!path.startsWith(root + "/")) {
    response.writeHead(404).end();
    return;
  }
  try {
    const body = await readFile(path);
    response.setHeader(
      "content-type",
      (
        {
          ".html": "text/html",
          ".js": "application/javascript",
          ".css": "text/css",
          ".json": "application/json",
        } as Record<string, string>
      )[extname(path)] || "application/octet-stream",
    );
    response.end(body);
  } catch {
    response.writeHead(404).end();
  }
});
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const address = server.address();
assert.ok(address && typeof address !== "string");
const profile = await mkdtemp(join(tmpdir(), "procinsh-static-browser-"));
const errors: string[] = [];
const chrome = launch(process.env.CHROME || "/usr/bin/google-chrome", [
  "--headless=new",
  "--no-sandbox",
  "--use-gl=angle",
  "--use-angle=swiftshader",
  "--enable-unsafe-swiftshader",
  "--disable-dev-shm-usage",
  "--remote-debugging-port=0",
  `--user-data-dir=${profile}`,
  "about:blank",
]);
let socket: WebSocket | undefined;
try {
  const debug = await until(
    () =>
      chrome.output.match(
        /ws:\/\/127\.0\.0\.1:\d+\/devtools\/browser\/[\w-]+/,
      )?.[0],
    "static Chrome",
  );
  const page = (await pages(new URL(debug).port)).find(
    (p) => p.type === "page",
  )!;
  const connection = await connectPage(page.webSocketDebuggerUrl, errors);
  socket = connection.socket;
  const { cdp, evaluate, waitFor } = connection;
  await cdp("Runtime.enable");
  await cdp("Page.enable");
  await cdp("Log.enable");
  await cdp("Page.addScriptToEvaluateOnNewDocument", {
    source: `window.EventSource=class { constructor(){throw Error('Static replay attempted SSE')} };`,
  });
  const url = `http://127.0.0.1:${address.port}/demo/replay/space/?recording=demo`;
  await cdp("Page.navigate", { url });
  await waitFor(
    "document.querySelector('#replay-status')?.textContent.includes('Sanitized demo')",
    "demo loads at deployment base path",
  );
  assert.equal(
    await evaluate("document.querySelector('#failure').hidden"),
    true,
  );
  await evaluate("document.querySelector('#play').click()");
  await waitFor(
    "Number(document.querySelector('#timeline').value)>200/30000",
    "virtual playback advances",
  );
  await evaluate("document.querySelector('#play').click()");
  const paused = await evaluate<number>(
    "Number(document.querySelector('#timeline').value)",
  );
  await delay(200);
  assert.equal(
    await evaluate("Number(document.querySelector('#timeline').value)"),
    paused,
  );
  await evaluate(
    "document.querySelector('#timeline').value=0.4;document.querySelector('#timeline').dispatchEvent(new Event('input'))",
  );
  assert.equal(
    await evaluate("Number(document.querySelector('#timeline').value)"),
    0.4,
  );
  await evaluate(
    "document.querySelector('select').value='0.1';document.querySelector('select').dispatchEvent(new Event('change'));document.querySelector('#play').click()",
  );
  await delay(300);
  const slow = await evaluate<number>(
    "Number(document.querySelector('#timeline').value)",
  );
  assert.ok(
    slow > 12000 / 30000 && slow < 12200 / 30000,
    `slow playback advances on virtual clock: ${slow}`,
  );
  await evaluate(
    "document.querySelector('#play').click();document.querySelector('#loop').click();document.querySelector('select').value='1';document.querySelector('select').dispatchEvent(new Event('change'));document.querySelector('#timeline').value=29900/30000;document.querySelector('#timeline').dispatchEvent(new Event('input'));document.querySelector('#play').click()",
  );
  await waitFor(
    "Number(document.querySelector('#timeline').value)<2000/30000 && document.querySelector('#play').textContent==='Pause'",
    "loop restarts and keeps playing",
  );
  await evaluate(
    "document.querySelector('#loop').click();document.querySelector('#timeline').value=29900/30000;document.querySelector('#timeline').dispatchEvent(new Event('input'));document.querySelector('#play').click()",
  );
  await waitFor(
    "Number(document.querySelector('#timeline').value)===1 && document.querySelector('#play').textContent==='Play'",
    "disabling loop stops at the end",
  );
  const timeOrigin = await evaluate<number>("performance.timeOrigin");
  await cdp("Page.reload");
  await waitFor(
    `performance.timeOrigin!==${timeOrigin} && document.querySelector('#replay-status')?.textContent.includes('Sanitized demo')`,
    "direct route refresh works",
  );
  await cdp("Page.navigate", {
    url: url.replace("recording=demo", "recording=https://example.com"),
  });
  await waitFor(
    "document.querySelector('#replay-status')?.textContent.includes('Unknown bundled recording')",
    "query ID is allowlisted",
  );
  assert.ok(requests.includes("/demo/recordings/demo.json"));
  assert.ok(
    requests.every((path) => !path.includes("/api/")),
    `no API requests: ${requests}`,
  );
  assert.deepEqual(errors, []);
  console.log(
    "Static SPACE replay passed: backend-free prefixed deployment, demo, refresh, seek, pause, slow playback, allowlisted IDs, zero API requests.",
  );
} finally {
  socket?.close();
  chrome.kill("SIGTERM");
  await delay(300);
  if (chrome.exitCode === null) chrome.kill("SIGKILL");
  server.closeAllConnections();
  server.close();
  await rm(profile, { recursive: true, force: true });
}
