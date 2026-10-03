import assert from 'node:assert/strict';
export async function checkLiveSamples({evaluate, waitFor, delay}) {
  assert.equal(await evaluate("document.querySelector('#snapshot, #auto-snapshot')"), null);
  await waitFor("document.querySelectorAll('#registers tr').length===18", 'automatic live registers');
  const before = await evaluate('liveSamples.find(t=>t.tid===selectedTid).sampled_at');
  await waitFor(`liveSamples.find(t=>t.tid===selectedTid)?.sampled_at > ${before}`, 'live sample advances');
  assert.equal(await evaluate("document.getElementById('sample-time')"), null);
  assert.equal(await evaluate("Object.hasOwn(liveSamples.find(t=>t.tid===selectedTid), 'sample_source')"), false);
  // Older samples retain their age in the thread list without a Stale label.
  await evaluate(`
    window.olderObservation = {...target, live_samples: liveSamples.map(t=>({...t, sampled_at:Date.now()-10000, sample_age_ms:10000}))};
    window.targetSources.at(-1).dispatchEvent(new MessageEvent('observation',{data:JSON.stringify(window.olderObservation)}));
  `);
  const threadText = await evaluate("document.getElementById('threads').textContent");
  assert.match(threadText, /10\.\ds/);
  assert.doesNotMatch(threadText, /Stale/);
  assert.equal(await evaluate("document.getElementById('sample-time')"), null);
  await delay(1200);
  await waitFor("liveSamples.find(t=>t.tid===selectedTid)?.sample_age_ms < 10000", 'live samples continue updating');
}
