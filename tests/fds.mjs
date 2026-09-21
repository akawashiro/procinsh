import assert from 'node:assert/strict';

export async function checkDescriptors({evaluate, waitFor, delay, choose, originalPid, ipcPid, peerPid}) {
  const back = async () => { await evaluate("document.getElementById('back').click()"); await waitFor("document.getElementById('inspector').hidden", 'back from FD target'); };
  await back(); await choose(ipcPid);
  assert.equal(await evaluate("document.getElementById('fds-panel').open"), false);
  await evaluate("document.querySelector('#fds-panel summary').click()");
  await waitFor("document.querySelector('#fds-entries tr[data-fd=\"60\"]') !== null", 'pipe/socket FD list');
  for (const fd of [60,61,62,64]) {
    assert.equal(await evaluate(`document.querySelector('#fds-entries tr[data-fd="${fd}"] [data-relation="peer"] button[data-pid="${peerPid}"]') !== null`), true, `peer link for FD ${fd}`);
  }
  assert.equal(await evaluate(`document.querySelector('#fds-entries tr[data-fd="61"] [data-relation="holder"] button[data-pid="${peerPid}"]') !== null`), true, 'shared endpoint separated from peer');
  assert.equal(await evaluate("document.querySelectorAll('#fds-entries tr[data-fd=\"65\"] [data-relation=\"peer\"]').length"), 0, 'listener is not paired with its clients');
  await evaluate("document.getElementById('fds-search').value = 'TCP'; document.getElementById('fds-search').dispatchEvent(new Event('input'))");
  assert.equal(await evaluate("document.querySelectorAll('#fds-entries tr').length"), 2);
  await evaluate("document.getElementById('fds-search').value = ''; document.getElementById('fds-search').dispatchEvent(new Event('input'))");
  await evaluate(`document.querySelector('#fds-entries tr[data-fd="61"] [data-relation="peer"] button[data-pid="${peerPid}"]').click()`);
  await waitFor(`document.getElementById('identity').textContent.includes('PID ${peerPid} /')`, 'click peer PID to inspect');
  assert.equal(await evaluate('location.pathname'), `/process/${peerPid}`);
  assert.equal(await evaluate("document.getElementById('fds-panel').open"), false);
  assert.equal(await evaluate("document.querySelectorAll('#fds-entries tr').length"), 0);
  await evaluate("document.querySelector('#fds-panel summary').click()");
  await waitFor(`document.querySelector('#fds-entries tr[data-fd="60"] [data-relation="peer"] button[data-pid="${ipcPid}"]') !== null`, 'reverse pipe peer');
  await evaluate(`document.querySelector('#fds-entries tr[data-fd="60"] [data-relation="peer"] button[data-pid="${ipcPid}"]').click()`);
  await waitFor(`document.getElementById('identity').textContent.includes('PID ${ipcPid} /')`, 'reverse PID navigation');
  await evaluate(`
    window.fdFetch = window.fetch; window.fdCalls = 0;
    window.fetch = async (...args) => {
      if (String(args[0]).includes('/api/target/fds?')) { window.fdCalls++; return new Response(JSON.stringify({error: 'FD permission denied (test)'}), {status: 422}); }
      return window.fdFetch(...args);
    };
    document.querySelector('#fds-panel summary').click();
  `);
  await waitFor("!document.getElementById('fds-error').hidden", 'FD permission error');
  await delay(1100); assert.equal(await evaluate('window.fdCalls'), 1, 'no automatic global FD scans');
  assert.equal(await evaluate("document.getElementById('error').hidden"), true);
  await evaluate('window.fetch = window.fdFetch');
  await back(); await choose(originalPid);
  console.log('FD checks passed: pipe/UNIX/TCP/UDP peers, shared holders, listener, search, PID navigation in both directions, permission error.');
}
