import assert from 'node:assert/strict';

export async function checkProcessSessions({cdp, evaluate, choose, until, delay, url, debugPort, originalPid, otherPid}) {
  const {targetId} = await cdp('Target.createTarget', {url});
  let socket;
  try {
    const page = await until(async () => {
      const pages = await (await fetch(`http://127.0.0.1:${debugPort}/json/list`)).json();
      return pages.find(p => p.id === targetId);
    }, 'second browser tab');
    socket = new WebSocket(page.webSocketDebuggerUrl);
    await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
    let sequence = 0;
    const pending = new Map();
    socket.onmessage = event => {
      const data = JSON.parse(event.data), task = pending.get(data.id);
      if (task) { pending.delete(data.id); data.error ? task.reject(data.error) : task.resolve(data.result); }
    };
    const call = (method, params) => new Promise((resolve, reject) => {
      const id = ++sequence;
      pending.set(id, {resolve, reject});
      socket.send(JSON.stringify({id, method, params}));
    });
    const second = async expression => {
      const result = await call('Runtime.evaluate', {expression, returnByValue: true, awaitPromise: true});
      if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
      return result.result.value;
    };
    await until(() => second("document.querySelectorAll('#process-list tr').length>2"), 'independent list');
    assert.equal(await second("document.getElementById('inspector').hidden"), true, 'root always shows list');
    await call('Page.navigate', {url: url+'/process/'+originalPid});
    await until(() => second(`document.getElementById('identity')?.textContent.includes('PID ${originalPid} /')`), 'second tab process');
    await evaluate("document.getElementById('back').click()");
    await choose(otherPid);
    await delay(300);
    assert.ok(await second(`document.getElementById('identity').textContent.includes('PID ${originalPid} /')`), 'other tab keeps its own process');
    await evaluate("document.getElementById('back').click()");
    const before = await second('target.observation.timestamp');
    await until(async () => (await second('target.observation.timestamp')) > before, 'other tab keeps observing after back');
    assert.equal(await evaluate("document.getElementById('inspector').hidden"), true);
    await choose(originalPid);
    console.log('Process sessions passed: independent tabs, root list, independent selection and disconnect.');
  } finally {
    socket?.close();
    await cdp('Target.closeTarget', {targetId});
  }
}
