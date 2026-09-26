import assert from 'node:assert/strict';
export async function checkSamples({evaluate, waitFor, delay, choose, otherPid, originalPid}) {
  await waitFor('captured?.threads.some(t => t.registers.length === 18)', 'live sample registers');
  const current = () => evaluate('captured.threads.find(t => t.tid === selectedTid)?.sampled_at_mono_ns');
  await evaluate("document.getElementById('freeze').click()");
  const before = await current();
  await delay(2200);
  assert.equal(await current(), before, 'Freeze holds displayed sample');
  assert.notEqual(await evaluate('liveSamples.threads.find(t => t.tid === selectedTid)?.sampled_at_mono_ns'), before, 'Freeze does not stop sampling');
  await evaluate("document.getElementById('freeze').click()");
  assert.notEqual(await current(), before);
  await evaluate(`const historyControl = document.getElementById('sample-history'); historyControl.selectedIndex = 1; historyControl.dispatchEvent(new Event('change'));`);
  assert.equal(await evaluate("document.getElementById('freeze').checked"), true, 'history selection freezes display');
  assert.match(await evaluate("document.getElementById('sample-time').textContent"), /monotonic/);
  await evaluate("document.getElementById('freeze').click()");
  // Deterministic UI degradation independent of host perf permissions.
  await evaluate(`window.savedSamples = structuredClone(liveSamples); const no = structuredClone(liveSamples); no.status='unavailable'; no.warnings=['permission denied (fixture)']; no.threads=[]; receiveSamples(no);`);
  assert.match(await evaluate("document.getElementById('samples-status').textContent"), /unavailable.*permission denied/);
  assert.match(await evaluate("document.getElementById('sample-time').textContent"), /No sample/);
  assert.ok(await evaluate("document.querySelectorAll('#maps tr').length > 0"), 'proc panels survive perf failure');
  await evaluate(`const stale=structuredClone(window.savedSamples); for(const s of stale.threads)s.sample_age_ms=10000; receiveSamples(stale);`);
  assert.match(await evaluate("document.getElementById('sample-time').textContent"), /stale/);
  await evaluate('receiveSamples(window.savedSamples)');
  await evaluate("window.oldSamplesSource=targetSource; document.getElementById('back').click()");
  await choose(otherPid);
  await evaluate(`window.oldSamplesSource.dispatchEvent(new MessageEvent('samples',{data:JSON.stringify(window.savedSamples)}))`);
  assert.notEqual(await evaluate('liveSamples?.process_id.pid'), originalPid, 'ignore old target samples');
  await evaluate("document.getElementById('back').click()"); await choose(originalPid);
  console.log('Samples passed: live data, Freeze, history, stale/no sample, unavailable, stale stream rejection.');
}
