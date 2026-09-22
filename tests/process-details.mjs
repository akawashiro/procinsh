import assert from 'node:assert/strict';

export async function checkProcessDetails({evaluate, waitFor, delay, choose, originalPid, otherPid}) {
  assert.equal(await evaluate("document.getElementById('signals-panel').open"), false);
  await evaluate("document.querySelector('#signals-panel summary').click()");
  await waitFor("document.getElementById('signals-entries').textContent.includes('SigBlk')", 'signal status load');
  assert.match(await evaluate("document.getElementById('signals-info').textContent"), /SigQ/);
  assert.match(await evaluate("document.getElementById('signals-entries').textContent"), /ShdPnd/);
  await evaluate("document.getElementById('signals-refresh').click()");
  await waitFor("!document.getElementById('signals-refresh').disabled", 'signal refresh');
  assert.equal(await evaluate("document.getElementById('signals-error').hidden"), true);
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
  await evaluate(`${entryRow}.querySelector('button').click()`);
  await waitFor("document.getElementById('memory').textContent.includes('|')", 'auxv pointer opens memory');
  assert.equal(await evaluate("document.getElementById('address').value"), address);
  assert.match(await evaluate("Array.from(document.querySelectorAll('#auxv-entries tr')).find(r => r.cells[0].textContent.startsWith('AT_EXECFN ')).cells[3].textContent"), /recursive/);

  await evaluate("window.detailTest.mode = 'denied'; document.getElementById('environment-refresh').click()");
  await waitFor("!document.getElementById('environment-error').hidden", 'environment permission error');
  assert.equal(await evaluate("document.getElementById('auxv-error').hidden"), true);
  assert.equal(await evaluate("document.getElementById('error').hidden"), true, 'details failure does not break inspector');
  assert.match(await evaluate("document.getElementById('environment-entries').textContent"), /PROCINSH_TEST_ENV/);
  await evaluate("window.detailTest.mode = 'empty'; document.getElementById('environment-refresh').click()");
  await waitFor("document.getElementById('environment-entries').textContent.includes('The environment is empty')", 'empty environment');
  await evaluate("window.detailTest.mode = 'hold'; document.getElementById('environment-refresh').click()");
  await waitFor('window.detailTest.release !== null', 'delayed environment response');
  await evaluate("document.getElementById('back').click()");
  await waitFor("document.getElementById('inspector').hidden", 'return during details read');
  await choose(otherPid);
  await evaluate('window.detailTest.release(); window.detailTest.release = null');
  await delay(200);
  assert.equal(await evaluate("document.querySelectorAll('#environment-entries tr').length"), 0, 'old target data must not appear');
  assert.equal(await evaluate("document.getElementById('auxv-panel').open"), false);
  assert.equal(await evaluate("document.querySelectorAll('#auxv-entries tr').length"), 0);
  await evaluate("window.fetch = window.detailFetch; document.getElementById('back').click()");
  await waitFor("document.getElementById('inspector').hidden", 'return after details tests');
  await choose(originalPid);
  console.log('Process details checks passed: on-demand environment/auxv, search, literal values, pointer navigation, errors, empty environment, stale response.');
}
