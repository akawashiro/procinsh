// Serve only replay build output at a deployment prefix; no application backend.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile, mkdtemp, rm } from "node:fs/promises";
import { join, resolve, extname } from "node:path";
import { tmpdir } from "node:os";
import { gzipSync } from "node:zlib";
import { connectPage, pages } from "./harness.js";
import { checkSpaceTour, checkSpaceTourUrl } from "./space-tour.js";
import { checkReplaySliderTravel } from "./replay-slider.js";
import { launch, until, delay } from "../support/runtime.js";

const root = resolve(import.meta.dirname, "../../dist-replay");
const requests: string[] = [];
let deploymentBase = "/";
let demoEncoding: "plain" | "gzip" | "http-gzip" | "asset" = "plain";
const server = createServer(async (request, response) => {
  const url = new URL(request.url!, "http://localhost");
  requests.push(url.pathname);
  if (!url.pathname.startsWith(deploymentBase)) {
    response.writeHead(404).end();
    return;
  }
  const path = resolve(
    root,
    url.pathname.slice(deploymentBase.length) +
      (url.pathname.endsWith("/") ? "index.html" : ""),
  );
  if (!path.startsWith(root + "/")) {
    response.writeHead(404).end();
    return;
  }
  try {
    const useFixture =
      path.endsWith("/recordings/chrome-ubuntu26.json.gz") &&
      demoEncoding !== "asset";
    const body = await readFile(
      useFixture
        ? resolve(import.meta.dirname, "../fixtures/space-recording.json")
        : path,
    );
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
    if (useFixture && demoEncoding !== "plain") {
      if (demoEncoding === "http-gzip")
        response.setHeader("Content-Encoding", "gzip");
      response.end(gzipSync(body));
    } else response.end(body);
  } catch {
    response.writeHead(404).end();
  }
});
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const address = server.address();
assert.ok(address && typeof address !== "string");
const profile = await mkdtemp(join(tmpdir(), "procinsh-static-browser-"));
const errors: string[] = [];
const chromium = launch(process.env.CHROMIUM || "/usr/bin/chromium", [
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
      chromium.output.match(
        /ws:\/\/127\.0\.0\.1:\d+\/devtools\/browser\/[\w-]+/,
      )?.[0],
    "static Chromium",
  );
  const page = (await pages(new URL(debug).port)).find(
    (p) => p.type === "page",
  )!;
  const connection = await connectPage(page.webSocketDebuggerUrl, errors);
  socket = connection.socket;
  const { cdp, evaluate, waitFor } = connection;
  await cdp("Runtime.enable");
  await cdp("Page.enable");
  await cdp("Network.enable");
  await cdp("Network.setCacheDisabled", { cacheDisabled: true });
  await cdp("Emulation.setDeviceMetricsOverride", {
    width: 1280,
    height: 900,
    deviceScaleFactor: 1,
    mobile: false,
  });
  await cdp("Log.enable");
  await cdp("Page.addScriptToEvaluateOnNewDocument", {
    source: `window.EventSource=class { constructor(){throw Error('Static replay attempted SSE')} };`,
  });
  for (const base of ["/", "/procinsh/demo/", "/software/procinsh/"]) {
    deploymentBase = base;
    demoEncoding = "plain";
    requests.length = 0;
    const url: string = `http://127.0.0.1:${address.port}${deploymentBase}replay/space/?recording=chrome-ubuntu26`;
    await cdp("Page.navigate", {
      url: `http://127.0.0.1:${address.port}${deploymentBase}`,
    });
    await waitFor(
      "document.querySelectorAll('main a').length===3",
      "demo landing page offers all recordings",
    );
    assert.deepEqual(
      await evaluate(
        "Array.from(document.querySelectorAll('main a'), a => new URL(a.href).pathname + new URL(a.href).search)",
      ),
      [
        `${deploymentBase}replay/space/?recording=chrome-ubuntu26`,
        `${deploymentBase}replay/space/?recording=firefox-ubuntu26`,
        `${deploymentBase}replay/space/?recording=linux-build-ubuntu26`,
      ],
    );
    await evaluate("document.querySelector('main a').click()");
    await waitFor(
      "document.querySelector('#play')?.textContent==='Pause'",
      "landing page link starts replay",
    );
    await cdp("Page.navigate", { url: url.split("?")[0] });
    await waitFor(
      "document.querySelector('#demo-recording')",
      "demo picker available without a recording",
    );
    assert.equal(
      await evaluate("document.querySelector('#demo-recording').value"),
      "",
    );
    assert.deepEqual(
      await evaluate(
        "Array.from(document.querySelectorAll('#demo-recording option'), option => option.textContent)",
      ),
      ["Open demo", "Chrome", "Firefox", "Linux kernel build"],
    );
    assert.equal(await evaluate("location.search"), "");
    for (const width of [1280, 390]) {
      await cdp("Emulation.setDeviceMetricsOverride", {
        width,
        height: 900,
        deviceScaleFactor: 1,
        mobile: false,
      });
      assert.equal(
        await evaluate(
          "document.querySelector('.left-hud').getBoundingClientRect().width",
        ),
        120,
      );
      assert.equal(
        await evaluate(
          "Array.from(document.querySelectorAll('.tools input, .tools button, .tools select')).filter(e => e.getBoundingClientRect().width > 0).every(e => { const r = e.getBoundingClientRect(); const p = document.querySelector('.left-hud').getBoundingClientRect(); return r.left >= p.left && r.right <= p.right; })",
        ),
        true,
      );
    }
    await cdp("Emulation.setDeviceMetricsOverride", {
      width: 1280,
      height: 900,
      deviceScaleFactor: 1,
      mobile: false,
    });
    await evaluate(
      "document.querySelector('#demo-recording').value='chrome-ubuntu26';document.querySelector('#demo-recording').dispatchEvent(new Event('change'))",
    );
    await waitFor(
      "document.querySelector('#replay-status')?.hidden && document.querySelector('#play').textContent==='Pause'",
      "demo loads at deployment base path",
    );
    assert.equal(
      await evaluate("document.querySelector('#failure').hidden"),
      true,
    );
    await waitFor(
      "document.querySelector('#loop').checked && document.querySelector('#play').textContent==='Pause' && Number(document.querySelector('#timeline').value)>0",
      "bundled recording autoplays with looping enabled",
    );
    assert.equal(await evaluate("location.href"), url);
    if (base === "/") await checkSpaceTour(cdp, evaluate);
    await checkReplaySliderTravel(cdp, evaluate);
    await evaluate("document.querySelector('#play').click()");
    await waitFor(
      "Number(document.querySelector('#timeline').value)>200/30000",
      "virtual playback advances",
    );
    await evaluate("document.querySelector('#play').click()");
    await evaluate("document.querySelector('#tour').click()");
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
      "document.querySelector('.replay-controls-row select').value='0.1';document.querySelector('.replay-controls-row select').dispatchEvent(new Event('change'));document.querySelector('#play').click()",
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
      "document.querySelector('#play').click();if(!document.querySelector('#loop').checked)document.querySelector('#loop').click();document.querySelector('.replay-controls-row select').value='1';document.querySelector('.replay-controls-row select').dispatchEvent(new Event('change'));document.querySelector('#timeline').value=29900/30000;document.querySelector('#timeline').dispatchEvent(new Event('input'));document.querySelector('#play').click()",
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
    assert.equal(
      await evaluate(
        "document.querySelector('#tour').getAttribute('aria-pressed')",
      ),
      "true",
      "tour survives replay pause, seek, loop and end",
    );
    await evaluate("document.querySelector('#tour').click()");
    const timeOrigin = await evaluate<number>("performance.timeOrigin");
    await cdp("Page.reload");
    await waitFor(
      `performance.timeOrigin!==${timeOrigin} && document.querySelector('#replay-status')?.hidden && document.querySelector('#loop').checked && document.querySelector('#play').textContent==='Pause'`,
      "direct route refresh works",
    );
    await cdp("Page.navigate", {
      url: url.replace(
        "recording=chrome-ubuntu26",
        "recording=https://example.com",
      ),
    });
    await waitFor(
      "document.querySelector('#replay-status')?.textContent.includes('Unknown bundled recording')",
      "query ID is allowlisted",
    );
    await evaluate(
      "document.querySelector('#demo-recording').value='chrome-ubuntu26';document.querySelector('#demo-recording').dispatchEvent(new Event('change'))",
    );
    await waitFor(
      "document.querySelector('#replay-status')?.hidden && document.querySelector('#loop').checked && document.querySelector('#play').textContent==='Pause'",
      "demo picker recovers from an unknown recording ID",
    );
    for (const encoding of ["gzip", "http-gzip"] as const) {
      demoEncoding = encoding;
      const previousOrigin = await evaluate<number>("performance.timeOrigin");
      await cdp("Page.navigate", { url });
      await waitFor(
        `performance.timeOrigin!==${previousOrigin} && document.querySelector('#replay-status')?.hidden && document.querySelector('#play').textContent==='Pause'`,
        `${encoding} URL replay avoids double decompression`,
      );
    }
    demoEncoding = "asset";
    for (const id of [
      "chrome-ubuntu26",
      "firefox-ubuntu26",
      "linux-build-ubuntu26",
    ]) {
      const previousOrigin = await evaluate<number>("performance.timeOrigin");
      await cdp("Page.navigate", {
        url: `http://127.0.0.1:${address.port}${deploymentBase}replay/space/?recording=${id}`,
      });
      await waitFor(
        `performance.timeOrigin!==${previousOrigin} && document.querySelector('#replay-status')?.hidden && document.querySelector('#play').textContent==='Pause' && document.querySelector('#loop').checked`,
        `real ${id} opens directly with looping playback`,
      );
      await evaluate(
        `document.querySelector('#demo-recording').value=${JSON.stringify(id)};document.querySelector('#demo-recording').dispatchEvent(new Event('change'))`,
      );
      await waitFor(
        `location.search==='?recording=${id}' && document.querySelector('#replay-status')?.hidden && document.querySelector('#play').textContent==='Pause' && Number(document.querySelector('#timeline').value)>0`,
        `real ${id} gzip plays without a backend`,
      );
      await evaluate(
        "document.querySelector('#timeline').value=0.5;document.querySelector('#timeline').dispatchEvent(new Event('input'))",
      );
      assert.equal(
        await evaluate("Number(document.querySelector('#timeline').value)"),
        0.5,
      );
      await evaluate("document.querySelector('#play').click()");
      await waitFor(
        "Number(document.querySelector('#timeline').value)>0.5",
        `real ${id} resumes after seek`,
      );
      assert.ok(requests.includes(`${deploymentBase}recordings/${id}.json.gz`));
    }
    const previousOrigin = await evaluate<number>("performance.timeOrigin");
    await cdp("Page.navigate", {
      url: url.replace("/replay/space/?", "/replay/space/index.html?"),
    });
    await waitFor(
      `performance.timeOrigin!==${previousOrigin} && document.querySelector('#replay-status')?.hidden && document.querySelector('#play').textContent==='Pause'`,
      "explicit index.html route loads the real recording",
    );
    assert.equal(
      (
        await fetch(
          `http://127.0.0.1:${address.port}${deploymentBase}recordings/demo.json`,
        )
      ).status,
      404,
    );
    assert.ok(
      requests.every((path) => !path.includes("/api/")),
      `no API requests: ${requests}`,
    );
    assert.ok(
      requests
        .filter((path) => path !== "/favicon.ico")
        .every((path) => path.startsWith(deploymentBase)),
      `all demo requests stay beneath ${deploymentBase}: ${requests}`,
    );
    await checkSpaceTourUrl(cdp, evaluate, waitFor, url);
    assert.deepEqual(errors, []);
    console.log(
      `Static SPACE replay passed at ${deploymentBase}: demo, refresh, seek, pause, slow playback, allowlisted IDs, zero API requests.`,
    );
  }
} finally {
  socket?.close();
  chromium.kill("SIGTERM");
  await delay(300);
  if (chromium.exitCode === null) chromium.kill("SIGKILL");
  server.closeAllConnections();
  server.close();
  await rm(profile, { recursive: true, force: true });
}
