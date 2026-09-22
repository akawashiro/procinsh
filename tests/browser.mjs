// Browser integration without npm dependencies. Requires Node >=22 and Chrome.
import {spawn} from 'node:child_process';
import {mkdtemp, rm, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import assert from 'node:assert/strict';
import {checkAutoSnapshot} from './auto-snapshot.mjs';
import {checkProcessDetails} from './process-details.mjs';
import {checkDescriptors} from './fds.mjs';
import {checkProcessSessions} from './process-sessions.mjs';

const children = [], errors = [];
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
function launch(program, args) {
  const p = spawn(program, args, {stdio: ['ignore', 'pipe', 'pipe'], env: {...process.env, PROCINSH_TEST_ENV: 'value=with\nline <b>literal</b>'}});
  p.output = ''; p.stdout.on('data', b => p.output += b); p.stderr.on('data', b => p.output += b);
  p.on('error', e => { p.output += e.message; });
  children.push(p); return p;
}
async function until(fn, label, timeout = 15000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) { const value = await fn(); if (value) return value; await delay(80); }
  throw new Error(`Timed out: ${label}`);
}
const profile = await mkdtemp(join(tmpdir(), 'procinsh-browser-'));
let socket;
try {
  const recursive = launch('tests/targets/bin/recursive', ['--allow-inspector']);
  const threads = launch('tests/targets/bin/threads', ['--allow-inspector']);
  const ipc = launch('tests/targets/bin/ipc', ['--allow-inspector']);
  const peerPid = Number(await until(() => ipc.output.match(/^\d+ 0x0 (\d+)\n/)?.[1], 'IPC fixtures'));
  await until(() => recursive.output.includes('\n') && threads.output.includes('\n'), 'test fixtures');
  const app = launch('target/debug/procinsh', ['--listen', '127.0.0.1:0', '--interval', '100ms']);
  const url = await until(() => app.output.match(/http:\/\/127\.0\.0\.1:\d+/)?.[0], 'HTTP server');
  const chrome = launch(process.env.CHROME || '/opt/google/chrome/google-chrome', ['--headless=new', '--no-sandbox', '--disable-gpu', '--disable-dev-shm-usage', '--no-first-run', '--remote-debugging-port=0', `--user-data-dir=${profile}`, 'about:blank']);
  const debugUrl = await until(() => chrome.output.match(/ws:\/\/127\.0\.0\.1:(\d+)\/devtools\/browser\/[\w-]+/)?.[0], 'Chrome DevTools');
  const debugPort = new URL(debugUrl).port;
  const pages = await (await fetch(`http://127.0.0.1:${debugPort}/json/list`)).json();
  socket = new WebSocket(pages.find(p => p.type === 'page').webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  let sequence = 0; const pending = new Map();
  socket.onmessage = event => {
    const data = JSON.parse(event.data);
    if (data.id) { const task = pending.get(data.id); if (task) { pending.delete(data.id); data.error ? task.reject(data.error) : task.resolve(data.result); } }
    if (data.method === 'Runtime.exceptionThrown') errors.push(data.params.exceptionDetails.text + ' ' + (data.params.exceptionDetails.exception?.description || ''));
    if (data.method === 'Log.entryAdded' && data.params.entry.level === 'error' && !data.params.entry.text.includes('favicon.ico')) errors.push(data.params.entry.text);
  };
  const cdp = (method, params = {}) => new Promise((resolve, reject) => { const id = ++sequence; pending.set(id, {resolve,reject}); socket.send(JSON.stringify({id,method,params})); });
  const evaluate = async expression => {
    const result = await cdp('Runtime.evaluate', {expression, returnByValue: true, awaitPromise: true});
    if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
    return result.result.value;
  };
  const waitFor = (expression, label) => until(() => evaluate(expression), label);
  await cdp('Runtime.enable'); await cdp('Log.enable'); await cdp('Page.enable');
  await cdp('Network.enable');
  const targetGets=[];
  const onRequest=event=>{
    const message=JSON.parse(event.data);
    if(message.method==='Network.requestWillBeSent'){
      const r=message.params.request;
      if(new URL(r.url).pathname==='/api/target')targetGets.push(r.url);
    }
  };
  socket.addEventListener('message',onRequest);
  await cdp('Page.addScriptToEvaluateOnNewDocument',{source:`
    window.targetSources=[];
    const Native=window.EventSource;
    window.EventSource=class extends Native {
      constructor(...args){super(...args);window.targetSources.push(this);
        if(String(args[0]).startsWith('/api/target/events?'))queueMicrotask(()=>this.dispatchEvent(new Event('error')));
      }
    };
  `});
  await cdp('Emulation.setDeviceMetricsOverride', {width: 1440, height: 1100, deviceScaleFactor: 1, mobile: false});
  await cdp('Page.navigate', {url});
  await waitFor("document.querySelectorAll('#process-list tr').length > 2", 'process explorer');
  await waitFor("document.getElementById('error').hidden", 'initial SSE recovers from connection error');
  assert.equal(await evaluate("document.getElementById('inspector').hidden"), true);
  assert.equal(await evaluate('document.title'), 'procinsh / list');
  assert.equal(await evaluate("document.querySelector('header #back').hidden"), true);
  async function choose(pid) {
    await evaluate(`document.getElementById('search').value = '${pid}'; document.getElementById('search').dispatchEvent(new Event('input'));`);
    await waitFor(`Array.from(document.querySelectorAll('#process-list tr')).some(r => r.cells[0].textContent === '${pid}')`, 'process search');
    await evaluate(`Array.from(document.querySelectorAll('#process-list tr')).find(r => r.cells[0].textContent === '${pid}').querySelector('button').click()`);
    await waitFor(`!document.getElementById('inspector').hidden && document.getElementById('identity').textContent.includes('PID ${pid} /')`, 'process selection');
  }
  await choose(recursive.pid);
  await evaluate("window.currentIdentity=JSON.stringify(target.summary.identity);const reused=structuredClone(target);reused.summary.identity.start_time_ticks++;window.targetSources.at(-1).dispatchEvent(new MessageEvent('observation',{data:JSON.stringify(reused)}))");
  assert.equal(await evaluate("JSON.stringify(target.summary.identity)"),await evaluate("window.currentIdentity"),'SSE cannot switch to a reused PID');
  await cdp('Page.navigate',{url:url+'/process/'+threads.pid});
  await waitFor(`document.getElementById('identity').textContent.includes('PID ${threads.pid} /')`,'direct URL selects requested target');
  await waitFor("window.targetSources.length===1",'direct URL opens one identified stream');
  await evaluate("window.oldSource=window.targetSources[0];document.getElementById('back').click()");
  await evaluate("window.oldSource.dispatchEvent(new MessageEvent('observation',{data:'invalid stale data'}))");
  assert.equal(await evaluate("document.getElementById('inspector').hidden"),true,'stale stream cannot reopen details');
  await choose(threads.pid);
  assert.ok(await evaluate(`document.getElementById('identity').textContent.includes('PID ${threads.pid} /')`),'stale initial stream is ignored');
  await cdp('Page.navigate',{url:url+'/process/2147483647'});
  await waitFor("document.getElementById('error').textContent.includes('Process exited')",'missing direct PID');
  assert.equal(await evaluate("document.getElementById('inspector').hidden"),true,'missing PID leaves list visible');
  await cdp('Page.navigate',{url:url+'/process/'+recursive.pid});
  await waitFor(`document.getElementById('identity').textContent.includes('PID ${recursive.pid} /')`,'direct URL original target');
  assert.equal(await evaluate("document.getElementById('target-status').hidden"), true);
  assert.equal(await evaluate("document.querySelector('header #back').hidden"), false);
  assert.equal(await evaluate("document.querySelector('header #back').textContent"), 'Back to process list');
  assert.equal(await evaluate('document.title'), await evaluate("'procinsh / ' + document.getElementById('target-name').textContent"));
  await checkProcessSessions({cdp,evaluate,choose,until,delay,url,debugPort,originalPid:recursive.pid,otherPid:threads.pid});
  await checkAutoSnapshot({evaluate, waitFor, delay, choose, otherPid: threads.pid, originalPid: recursive.pid});
  await checkProcessDetails({evaluate, waitFor, delay, choose, otherPid: threads.pid, originalPid: recursive.pid});
  await checkDescriptors({evaluate, waitFor, delay, choose, originalPid: recursive.pid, ipcPid: ipc.pid, peerPid});
  await waitFor("document.querySelectorAll('#maps tr').length > 5", 'memory maps');
  await evaluate("document.getElementById('snapshot').click()");
  await waitFor("document.querySelectorAll('#registers tr').length === 18", 'register snapshot');
  assert.match(await evaluate("document.getElementById('call-stack').textContent"), /foo/);
  assert.match(await evaluate("document.getElementById('call-stack').textContent"), /recursive\.c/);
  await waitFor("document.querySelectorAll('#disassembly tr').length > 0", 'disassembly from captured RIP');
  const ripText = () => evaluate("Array.from(document.querySelectorAll('#registers tr')).find(r => r.cells[0].textContent === 'RIP').cells[1].textContent");
  assert.equal(await evaluate("document.querySelector('#disassembly .current-instruction').cells[1].textContent"), await ripText());
  assert.match(await evaluate("document.querySelector('#disassembly tr').cells[2].textContent"), /^[0-9a-f]{2}( [0-9a-f]{2})*$/);
  assert.ok((await evaluate("document.querySelector('#disassembly tr').cells[3].textContent")).length > 0);
  await evaluate("document.querySelector('#disassembly .current-instruction button').click()");
  await waitFor("document.getElementById('memory').textContent.includes('|')", 'instruction address to memory');
  assert.equal(await evaluate("document.getElementById('address').value"), await ripText());
  await evaluate("Array.from(document.querySelectorAll('#registers tr')).find(r => r.cells[0].textContent === 'RSP').querySelector('button').click()");
  await waitFor("document.getElementById('memory').textContent.includes('|')", 'register to memory navigation');
  assert.match(await evaluate("document.getElementById('memory-info').textContent"), /256 \/ 256 bytes/);
  const png = await cdp('Page.captureScreenshot', {format: 'png', captureBeyondViewport: true});
  await writeFile('target/browser-inspector.png', Buffer.from(png.data, 'base64'));
  await cdp('Emulation.setDeviceMetricsOverride', {width: 390, height: 844, deviceScaleFactor: 1, mobile: true});
  assert.equal(await evaluate('document.documentElement.scrollWidth <= 390'), true, 'mobile layout must not overflow');
  await evaluate("document.getElementById('back').click()");
  await waitFor("document.getElementById('inspector').hidden", 'return to explorer');
  assert.equal(await evaluate("document.querySelectorAll('#disassembly tr').length"), 0);
  await choose(threads.pid);
  await waitFor("document.querySelectorAll('#threads tr').length >= 6", 'thread view');
  await evaluate("document.getElementById('snapshot').click()");
  await waitFor("document.querySelectorAll('#registers tr').length === 18", 'multi-thread snapshot');
  await evaluate("document.querySelectorAll('#threads button')[1].click()");
  assert.match(await evaluate("document.getElementById('stack-tid').textContent"), /TID \d+/);
  assert.equal(await evaluate("document.querySelector('#disassembly .current-instruction').cells[1].textContent"), await ripText());
  assert.equal(await evaluate("document.getElementById('error').hidden"), true);
  await evaluate("document.getElementById('auto-snapshot').click()");
  threads.kill('SIGTERM');
  await waitFor("document.getElementById('target-status').textContent.includes('Process exited')", 'process exit');
  assert.equal(await evaluate("document.getElementById('target-status').hidden"), false);
  assert.equal(await evaluate("document.getElementById('auto-snapshot').checked"), false);
  assert.ok(await evaluate("window.targetSources.at(-1).readyState===EventSource.CLOSED"),'process exit closes the stream');
  await evaluate("document.getElementById('back').click()");
  await choose(recursive.pid);
  // An open SSE connection must not hang graceful shutdown.
  app.kill('SIGTERM'); await until(() => app.exitCode !== null, 'shutdown with active SSE', 5000);
  const removed = launch('target/debug/procinsh', ['--pid', String(recursive.pid)]);
  await until(()=>removed.exitCode!==null,'removed CLI option');
  assert.notEqual(removed.exitCode,0);
  // Connection failures caused by deliberately shutting down the first server are expected.
  assert.deepEqual(errors.filter(e => !/ERR_CONNECTION_REFUSED|Failed to load resource/.test(e)), []);
  assert.deepEqual(targetGets,[],'UI never requests removed /api/target');
  console.log('Browser checks passed: explorer, search, selection, SSE initialization/direct URLs/stale events, snapshots, source lines, memory, mobile layout, thread switching, process exit, graceful shutdown, removed --pid.');
} finally {
  socket?.close();
  for (const child of children.reverse()) if (child.exitCode === null && child.signalCode === null) child.kill('SIGTERM');
  await delay(300);
  for (const child of children) if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
  await rm(profile, {recursive: true, force: true}).catch(() => {});
}
