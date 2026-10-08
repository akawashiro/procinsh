import assert from 'node:assert/strict';

export async function checkProcessDetails({evaluate, waitFor, delay, choose, originalPid, otherPid}) {
  assert.equal(await evaluate("document.getElementById('signals-panel')"), null);
  for (const kind of ['environment', 'auxv', 'fds'])
    assert.equal(await evaluate(`document.getElementById('${kind}-refresh')`), null);
  assert.equal(await evaluate("document.getElementById('environment-panel').open"), false);
  assert.equal(await evaluate("performance.getEntriesByType('resource').filter(e => /\\/api\\/processes\\/(environment|auxv)\\?/.test(e.name)).length"), 0, 'details must not load until opened');
  await evaluate(`
    window.detailTest = {calls: {environment: 0, auxv: 0}, mode: 'normal', release: null};
    window.detailFetch = window.fetch;
    window.fetch = async (...args) => {
      const match = String(args[0]).match(/\\/api\\/processes\\/(environment|auxv)\\?/);
      if (!match) return window.detailFetch(...args);
      const kind = match[1], t = window.detailTest; t.calls[kind]++;
      if (t.mode === 'denied' && kind === 'environment') return new Response(JSON.stringify({error: 'Permission denied (test)'}), {status: 422});
      const response = await window.detailFetch(...args);
      if (t.mode === 'hold') await new Promise(resolve => { t.release = resolve; });
      if (t.mode === 'empty' && kind === 'environment') { const data = await response.json(); data.entries = []; return new Response(JSON.stringify(data)); }
      return response;
    };
  `);
  const load = kind => evaluate(`document.querySelector('#${kind}-panel summary').click()`);
  await load('environment');
  await waitFor("document.getElementById('environment-entries').textContent.includes('PROCINSH_TEST_ENV')", 'environment load');
  await evaluate("document.getElementById('environment-search').value = 'PROCINSH_TEST_ENV'; document.getElementById('environment-search').dispatchEvent(new Event('input'))");
  assert.equal(await evaluate("document.querySelectorAll('#environment-entries tr').length"), 1);
  assert.equal(await evaluate("document.querySelector('#environment-entries tr').cells[1].textContent"), 'value=with\nline <b>literal</b>');
  assert.equal(await evaluate("document.querySelector('#environment-entries b')"), null, 'environment values are literal text');
  await delay(1100); assert.equal(await evaluate('window.detailTest.calls.environment'), 1, 'SSE must not reload environment');
  await load('auxv');
  await waitFor("document.getElementById('auxv-entries').textContent.includes('AT_PAGESZ')", 'auxiliary vector load');
  const entryRow = "Array.from(document.querySelectorAll('#auxv-entries tr')).find(r => r.cells[0].textContent.startsWith('AT_ENTRY '))";
  const address = await evaluate(`${entryRow}.cells[1].textContent`);
  assert.match(address, /^0x[0-9a-f]+$/i);
  assert.equal(await evaluate(`${entryRow}.querySelector('button')`), null, 'auxv addresses are text');
  assert.match(await evaluate("Array.from(document.querySelectorAll('#auxv-entries tr')).find(r => r.cells[0].textContent.startsWith('AT_EXECFN ')).cells[3].textContent"), /recursive/);

  await waitFor('window.detailTest.calls.environment >= 2 && window.detailTest.calls.auxv >= 2', 'open panels automatically refresh');
  assert.match(await evaluate("document.getElementById('environment-info').textContent"), /auto 5s/);
  await evaluate("document.querySelector('#auxv-panel summary').click()");
  await delay(200);
  const closedAuxvCalls = await evaluate('window.detailTest.calls.auxv');
  await evaluate("window.detailTest.mode = 'denied'");
  await waitFor("!document.getElementById('environment-error').hidden", 'environment permission error');
  assert.equal(await evaluate("document.getElementById('auxv-error').hidden"), true);
  assert.equal(await evaluate("document.getElementById('error').hidden"), true, 'details failure does not break inspector');
  assert.match(await evaluate("document.getElementById('environment-entries').textContent"), /PROCINSH_TEST_ENV/);
  assert.equal(await evaluate('window.detailTest.calls.auxv'), closedAuxvCalls, 'closed panels stop refreshing');
  await evaluate("window.detailTest.mode = 'empty'");
  await waitFor("document.getElementById('environment-entries').textContent.includes('The environment is empty')", 'empty environment');
  await evaluate("window.detailTest.mode = 'hold'; window.detailTest.callsBeforeHold = window.detailTest.calls.environment");
  await waitFor('window.detailTest.release !== null', 'delayed environment response');
  await delay(5500);
  assert.equal(await evaluate('window.detailTest.calls.environment'), await evaluate('window.detailTest.callsBeforeHold + 1'), 'busy reads skip automatic updates');
  await evaluate("window.dispatchEvent(new Event('pagehide'))");
  const hiddenCalls = await evaluate('window.detailTest.calls.environment');
  await evaluate('window.detailTest.release(); window.detailTest.release = null');
  await delay(5500);
  assert.equal(await evaluate('window.detailTest.calls.environment'), hiddenCalls, 'pagehide stops updates');
  assert.match(await evaluate("document.getElementById('environment-entries').textContent"), /The environment is empty/, 'pagehide invalidates pending response');
  await evaluate("window.dispatchEvent(new PageTransitionEvent('pageshow',{persisted:true}))");
  await waitFor("document.getElementById('inspector')?.hidden===false && document.getElementById('error')?.hidden===true", 'resume after pagehide');
  await evaluate("window.detailTest.mode = 'hold'");
  await load('environment');
  await waitFor('window.detailTest.release !== null', 'delayed response before target switch');
  await evaluate("addEventListener('pagehide',()=>{window.detailTest.release?.();window.fetch=window.detailFetch},{once:true});document.getElementById('back').click()");
  await waitFor("document.querySelectorAll('#process-list tr').length>2", 'return before target switch');
  await choose(otherPid);
  await delay(200);
  assert.equal(await evaluate("document.querySelectorAll('#environment-entries tr').length"), 0, 'old target data must not appear');
  assert.equal(await evaluate("document.getElementById('auxv-panel').open"), false);
  assert.equal(await evaluate("document.querySelectorAll('#auxv-entries tr').length"), 0);
  await evaluate("document.getElementById('back').click()");
  await waitFor("document.querySelectorAll('#process-list tr').length>2", 'return after details tests');
  await choose(originalPid);
  console.log('Process details checks passed: automatic environment/auxv, search, literal values, address display, errors, empty environment, stale response.');
}
