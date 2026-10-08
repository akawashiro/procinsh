import type { ProcessChecks } from "./harness.js";
import assert from "node:assert/strict";
export async function checkLiveSamples({
  evaluate,
  waitFor,
  delay,
}: ProcessChecks) {
  assert.equal(
    await evaluate("document.querySelector('#snapshot, #auto-snapshot')"),
    null,
  );
  await waitFor(
    "document.querySelectorAll('#registers tr').length===18",
    "automatic live registers",
  );
  assert.equal(
    await evaluate("document.getElementById('disasm-location')"),
    null,
  );
  assert.doesNotMatch(
    await evaluate<string>(
      "document.querySelector('.disassembly').textContent",
    ),
    /RIP marks the sampled instruction/,
  );
  await waitFor(
    "document.querySelectorAll('#disassembly tr').length > 0",
    "live disassembly instructions",
  );
  const tid = await evaluate(
    "Number(document.getElementById('stack-tid').textContent.match(/TID (\\d+)/)[1])",
  );
  const sample = `window.lastObservation.live_samples.find(t=>t.tid===${tid})`;
  const before = await evaluate(`${sample}.sampled_at`);
  await waitFor(
    `${sample}?.sampled_at > ${before}`,
    "selected thread live sample advances",
  );
  assert.equal(await evaluate("document.getElementById('sample-time')"), null);
  assert.ok(
    await evaluate(
      "window.lastObservation.live_samples.every(t=>!Object.hasOwn(t,'sample_source'))",
    ),
  );
  // Older samples retain their age in the thread list without a Stale label.
  await evaluate(`
    window.olderObservation = {...window.lastObservation, live_samples: window.lastObservation.live_samples.map(t=>({...t, sampled_at:Date.now()-10000, sample_age_ms:10000}))};
    window.targetSources.at(-1).dispatchEvent(new MessageEvent('observation',{data:JSON.stringify(window.olderObservation)}));
  `);
  const threadText = await evaluate<string>(
    "document.getElementById('threads').textContent",
  );
  assert.match(threadText, /10\.\ds/);
  assert.doesNotMatch(threadText, /Stale/);
  assert.equal(await evaluate("document.getElementById('sample-time')"), null);
  await delay(1200);
  await waitFor(
    `${sample}?.sample_age_ms < 10000`,
    "selected thread live samples continue updating",
  );
}
