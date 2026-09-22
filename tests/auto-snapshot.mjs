import assert from 'node:assert/strict';

export async function checkAutoSnapshot({evaluate, waitFor, delay, choose, otherPid, originalPid}) {
  // Delay only the response to real fixture captures: the tracee has already
  // resumed, while the UI must still treat the request as in flight.
  await evaluate(`
    window.snapshotTest = {calls: 0, completed: 0, active: 0, maximum: 0, mode: 'normal', release: null};
    window.originalFetch = window.fetch;
    window.fetch = async (...args) => {
      if (args[0] !== '/api/processes/snapshot') return window.originalFetch(...args);
      const t = window.snapshotTest; t.calls++; t.active++; t.maximum = Math.max(t.maximum, t.active);
      try {
        if (t.mode === 'denied') return new Response(JSON.stringify({error: 'PTRACE_SEIZE: Operation not permitted (test)'}), {status: 422});
        if (t.mode === 'network') throw new TypeError('Network unavailable (test)');
        const response = await window.originalFetch(...args);
        if (t.mode === 'hold') await new Promise(resolve => { t.release = resolve; });
        t.completed++; return response;
      } finally { t.active--; }
    };
  `);
  const toggle = () => evaluate("document.getElementById('auto-snapshot').click()");
  const checked = () => evaluate("document.getElementById('auto-snapshot').checked");
  const calls = () => evaluate('window.snapshotTest.calls');
  const idle = () => waitFor('window.snapshotTest.active === 0 && !snapshotBusy', 'snapshot idle');
  assert.equal(await checked(), false);
  await toggle();
  assert.equal(await evaluate("document.getElementById('snapshot').disabled"), true);
  await waitFor('window.snapshotTest.completed >= 3', 'repeated automatic snapshots');
  await idle();
  assert.equal(await checked(), true);
  assert.equal(await evaluate("document.getElementById('snapshot').disabled"), true, 'manual capture stays disabled between automatic captures');
  await waitFor("document.querySelector('#disassembly .current-instruction')?.cells[1].textContent === Array.from(document.querySelectorAll('#registers tr')).find(r => r.cells[0].textContent === 'RIP')?.cells[1].textContent", 'automatic disassembly matches RIP');
  await toggle(); await idle();
  assert.equal(await evaluate("document.getElementById('snapshot').disabled"), false, 'manual capture is enabled after auto capture stops');
  let count = await calls(); await delay(1200); assert.equal(await calls(), count, 'OFF must stop requests');

  await evaluate("window.snapshotTest.mode = 'hold'"); await toggle();
  await waitFor('window.snapshotTest.release !== null', 'held response');
  count = await calls(); await delay(2200);
  assert.equal(await calls(), count, 'slow capture must skip ticks');
  assert.equal(await evaluate('window.snapshotTest.maximum'), 1, 'never overlap captures');
  const before = await evaluate('captured.captured_at');
  await toggle();
  await evaluate('window.snapshotTest.release(); window.snapshotTest.release = null'); await idle();
  assert.ok(await evaluate('captured.captured_at') > before, 'stopping still accepts final result for same target');
  await delay(1200); assert.equal(await calls(), count);

  const last = await evaluate("document.getElementById('registers').textContent");
  const lastDisassembly = await evaluate("document.getElementById('disassembly').textContent");
  for (const mode of ['denied', 'network']) {
    await evaluate(`window.snapshotTest.mode = '${mode}'`); await toggle();
    await waitFor("!document.getElementById('auto-snapshot').checked && !document.getElementById('error').hidden", `${mode} stops automatic capture`);
    await idle(); count = await calls(); await delay(1100); assert.equal(await calls(), count);
    assert.equal(await evaluate("document.getElementById('registers').textContent"), last, 'keep last successful snapshot');
    assert.equal(await evaluate("document.getElementById('disassembly').textContent"), lastDisassembly, 'keep last captured disassembly');
  }
  await evaluate("window.snapshotTest.mode = 'normal'"); await toggle(); await idle();
  await evaluate("Object.defineProperty(document, 'hidden', {configurable: true, value: true}); document.dispatchEvent(new Event('visibilitychange'))");
  assert.equal(await checked(), false); count = await calls();
  await evaluate("delete document.hidden; document.dispatchEvent(new Event('visibilitychange'))");
  await delay(1100); assert.equal(await calls(), count, 'visible again must not restart');

  // Change targets while a response from the old target is delayed.
  await evaluate("window.snapshotTest.mode = 'hold'"); await toggle();
  await waitFor('window.snapshotTest.release !== null', 'response before target change');
  await evaluate("document.getElementById('back').click()");
  await waitFor("document.getElementById('inspector').hidden", 'back while capturing');
  await choose(otherPid); assert.equal(await checked(), false);
  await evaluate('window.snapshotTest.release(); window.snapshotTest.release = null'); await idle();
  assert.equal(await evaluate("document.querySelectorAll('#registers tr').length"), 0, 'discard previous target result');
  assert.equal(await evaluate("document.querySelectorAll('#disassembly tr').length"), 0, 'discard previous target disassembly');
  count = await calls(); await delay(1100); assert.equal(await calls(), count);
  await evaluate("window.snapshotTest.mode = 'normal'");
  await waitFor("document.querySelectorAll('#threads button').length >= 2", 'worker threads');
  await evaluate("document.querySelectorAll('#threads button')[1].click()");
  const selected = await evaluate("document.querySelector('#threads tr.selected button').textContent");
  const completed = await evaluate('window.snapshotTest.completed');
  await toggle(); await waitFor(`window.snapshotTest.completed >= ${completed + 2}`, 'automatic worker snapshots');
  assert.equal(await evaluate("document.querySelector('#threads tr.selected button').textContent"), selected, 'retain selected thread');
  await toggle(); await idle();
  await evaluate("document.getElementById('back').click()");
  await waitFor("document.getElementById('inspector').hidden", 'return for manual regression tests');
  await choose(originalPid);
  await evaluate('window.fetch = window.originalFetch');
  console.log('Auto snapshot checks passed: cadence, stop, slow response, non-overlap, errors, hidden tab, stale result, thread selection.');
}
