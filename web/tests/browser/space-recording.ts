// Exercise browser downloads and local File API loading using the live binary.
import assert from "node:assert/strict";
import type { Cdp, Evaluate, WaitFor } from "./harness.js";
import { parseRecording } from "../../src/space/recording.js";
import { delay } from "../support/runtime.js";

export async function checkSpaceRecording(
  cdp: Cdp,
  evaluate: Evaluate,
  waitFor: WaitFor,
  base: string,
) {
  await cdp("Page.navigate", { url: base + "/space" });
  await waitFor(
    "document.querySelector('#record')?.disabled===false",
    "recording baseline ready",
  );
  await evaluate(`window.downloads=[]; window.downloadBlobs=[];
    window.recordingWireEvents=0;
    for (const name of ['snapshot','activity'])
      window.spaceTestSources.at(-1).addEventListener(name,()=>window.recordingWireEvents++);
    const create=URL.createObjectURL.bind(URL);
    URL.createObjectURL=blob=>{window.downloadBlobs.push(blob);return create(blob)};
    HTMLAnchorElement.prototype.click=function(){window.downloads.push({name:this.download,href:this.href})};
    document.querySelector('#record').click()`);
  await waitFor(
    "window.recordingWireEvents>0",
    "live event captured during recording",
  );
  assert.match(
    await evaluate<string>(
      "document.querySelector('#recording-status').textContent",
    ),
    /REC/,
  );
  await evaluate("document.querySelector('#download-recording').click()");
  await waitFor("window.downloads.length===1", "one recording download");
  const text = await evaluate<string>("window.downloadBlobs[0].text()");
  const recording = parseRecording(text);
  assert.ok(recording.initial_snapshot.processes.length > 0);
  assert.ok(recording.events.length > 0);
  assert.equal(
    (await evaluate<string>("window.downloads[0].name")).endsWith(".json"),
    true,
  );
  assert.equal(await evaluate("window.downloadBlobs.length"), 1);
  await evaluate("document.querySelector('#record').click()");
  await delay(150);
  await evaluate(
    "window.spaceTestSources.at(-1).dispatchEvent(new Event('error'))",
  );
  await waitFor(
    "document.querySelector('#download-recording').textContent==='Save file'",
    "connection interruption retains recording",
  );
  assert.match(
    await evaluate<string>(
      "document.querySelector('#recording-status').textContent",
    ),
    /interrupted/,
  );
  await evaluate("document.querySelector('#download-recording').click()");
  assert.equal(await evaluate("window.downloads.length"), 2);
  const interrupted = parseRecording(
    await evaluate<string>("window.downloadBlobs[1].text()"),
  );
  assert.match(interrupted.end_reason, /interrupted/);
  await evaluate("document.querySelector('button#open-recording').click()");
  await waitFor(
    "document.querySelector('#replay-status')",
    "direct replay route",
  );
  await evaluate(`window.EventSource=class{constructor(){throw Error('Replay opened SSE')}};
    const file = new File([${JSON.stringify(text)}], 'local-recording.json', {type:'application/json'});
    const transfer = new DataTransfer();transfer.items.add(file);
    const input=document.querySelector('input[type=file]');input.files=transfer.files;
    input.dispatchEvent(new Event('change'))`);
  await waitFor(
    "document.querySelector('#replay-status')?.hidden && document.querySelector('#play').textContent==='Pause'",
    "downloaded JSON opens unchanged",
  );
  assert.equal(
    await evaluate(
      "document.querySelector('#loop').checked && document.querySelector('#play').textContent==='Pause'",
    ),
    true,
    "local recording autoplays in a loop",
  );
  const endTime = recording.events.at(-1)?.at_ms ?? 0;
  assert.equal(
    await evaluate("Number(document.querySelector('#timeline').max)"),
    1,
  );
  await evaluate(
    "const timeline=document.querySelector('#timeline');timeline.value=timeline.max;timeline.dispatchEvent(new Event('input'))",
  );
  assert.equal(
    await evaluate("document.querySelector('#timeline').valueAsNumber"),
    1,
    "the slider reaches its right edge",
  );
  assert.equal(
    await evaluate(
      "import(window.spaceTestModule).then(module => module.replayTime)",
    ),
    endTime,
    "the right edge seeks to the exact final event, including fractional milliseconds",
  );
  await evaluate("document.querySelector('#play').click()");
  await delay(100);
  await evaluate("document.querySelector('#play').click()");
  assert.equal(
    await evaluate("document.querySelector('#play').textContent"),
    "Play",
  );
  await evaluate("document.querySelector('#tour').click()");
  const readCamera = () =>
    evaluate<{ position: number[]; target: number[] }>(
      "import(window.spaceTestModule).then(m => m.cameraView())",
    );
  const pausedTime = await evaluate(
    "import(window.spaceTestModule).then(m => m.replayTime)",
  );
  const cameraBefore = await readCamera();
  await delay(1200);
  const cameraAfter = await readCamera();
  assert.notDeepEqual(
    cameraAfter.position,
    cameraBefore.position,
    "camera advances while replay is paused",
  );
  assert.equal(
    await evaluate("import(window.spaceTestModule).then(m => m.replayTime)"),
    pausedTime,
  );
  const seek = await evaluate<{
    before: unknown;
    after: unknown;
  }>(`(async () => {
    const m = await import(window.spaceTestModule);
    const before = m.cameraView();
    const timeline = document.querySelector('#timeline');
    timeline.value = 0;
    timeline.dispatchEvent(new Event('input'));
    return { before, after: m.cameraView() };
  })()`);
  assert.deepEqual(
    seek.after,
    seek.before,
    "replay seek preserves the camera pose in the same frame",
  );
  assert.equal(
    await evaluate(
      "document.querySelector('#tour').getAttribute('aria-pressed')",
    ),
    "true",
  );
  await evaluate("document.querySelector('#tour').click()");
  await evaluate("document.querySelector('#loop').click()");
  await evaluate(
    `document.dispatchEvent(new DragEvent('drop',{dataTransfer:(()=>{const t=new DataTransfer();t.items.add(new File(['{}'],'bad.json'));return t})()}))`,
  );
  await waitFor(
    "document.querySelector('#replay-status').textContent.includes('Invalid SPACE recording')",
    "invalid dropped JSON rejected",
  );
  await evaluate(
    `document.dispatchEvent(new DragEvent('drop',{dataTransfer:(()=>{const t=new DataTransfer();t.items.add(new File([${JSON.stringify(text)}],'dropped.json'));return t})()}))`,
  );
  await waitFor(
    "document.querySelector('#replay-status')?.hidden && document.querySelector('#play').textContent==='Pause'",
    "another recording opens by drop",
  );
  assert.equal(
    await evaluate(
      "document.querySelector('#loop').checked && document.querySelector('#play').textContent==='Pause'",
    ),
    true,
    "opening another recording re-enables loop autoplay",
  );
  for (const method of ["picker", "drop"]) {
    await evaluate(`(async()=>{
      const gzip = await new Response(new Blob([${JSON.stringify(text)}]).stream().pipeThrough(new CompressionStream('gzip'))).blob();
      const transfer = new DataTransfer();
      transfer.items.add(new File([gzip], 'compressed-${method}.json.gz', {type:'application/gzip'}));
      if (${JSON.stringify(method)} === 'picker') {
        const input=document.querySelector('input[type=file]'); input.files=transfer.files;
        input.dispatchEvent(new Event('change'));
      } else document.dispatchEvent(new DragEvent('drop',{dataTransfer:transfer}));
    })()`);
    await waitFor(
      `document.querySelector('#replay-status')?.hidden && document.querySelector('#play').textContent==='Pause'`,
      `gzip ${method} replays`,
    );
  }
  assert.equal(
    await evaluate(
      'document.querySelectorAll(\'a[href^="/process/"],a[href="/list"],a[href="/space"]\').length',
    ),
    0,
  );
  await cdp("Page.navigate", { url: base + "/space" });
  await waitFor(
    "document.querySelector('#demo-recording')",
    "live demo picker",
  );
  await evaluate(
    "document.querySelector('#demo-recording').value='chromium-ubuntu26';document.querySelector('#demo-recording').dispatchEvent(new Event('change'))",
  );
  await waitFor(
    "location.search==='?recording=chromium-ubuntu26' && document.querySelector('#replay-status')?.hidden && document.querySelector('#loop').checked && document.querySelector('#play').textContent==='Pause'",
    "live Open demo navigates to looping demo playback",
  );
}
