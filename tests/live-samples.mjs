import assert from 'node:assert/strict';
export async function checkLiveSamples({evaluate, waitFor, delay}) {
  assert.equal(await evaluate("document.querySelector('#snapshot, #auto-snapshot')"), null);
  await waitFor("document.querySelectorAll('#registers tr').length===18", 'automatic live registers');
  const before = await evaluate('liveSamples.find(t=>t.tid===selectedTid).sampled_at');
  await waitFor(`liveSamples.find(t=>t.tid===selectedTid)?.sampled_at > ${before}`, 'live sample advances');
  assert.match(await evaluate("document.getElementById('sample-time').textContent"), /Latest observed sample/);
  assert.equal(await evaluate("Object.hasOwn(liveSamples.find(t=>t.tid===selectedTid), 'sample_source')"), false);
  // Replay a stale sample through the same SSE path used by the server.
  await evaluate(`
    window.staleObservation = {...target, live_samples: liveSamples.map(t=>({...t, sampled_at:Date.now()-10000, sample_age_ms:10000}))};
    window.targetSources.at(-1).dispatchEvent(new MessageEvent('observation',{data:JSON.stringify(window.staleObservation)}));
  `);
  assert.match(await evaluate("document.getElementById('sample-time').textContent"), /Stale/);
  await delay(1200);
  await waitFor("!document.getElementById('sample-time').textContent.includes('Stale')", 'live samples recover');
}
