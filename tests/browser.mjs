// Browser integration without npm dependencies. Requires Node >=22 and Chrome.
import {spawn} from 'node:child_process';
import {mkdtemp, rm, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import assert from 'node:assert/strict';
import {checkAutoSnapshot} from './auto-snapshot.mjs';
import {checkProcessDetails} from './process-details.mjs';
import {checkDescriptors} from './fds.mjs';

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
  await cdp('Emulation.setDeviceMetricsOverride', {width: 1440, height: 1100, deviceScaleFactor: 1, mobile: false});
  await cdp('Page.navigate', {url});
  await waitFor("document.querySelectorAll('#process-list tr').length > 2", 'process explorer');
  assert.equal(await evaluate("document.getElementById('inspector').hidden"), true);
  async function choose(pid) {
    await evaluate(`document.getElementById('search').value = '${pid}'; document.getElementById('search').dispatchEvent(new Event('input'));`);
    await waitFor(`Array.from(document.querySelectorAll('#process-list tr')).some(r => r.cells[0].textContent === '${pid}')`, 'process search');
    await evaluate(`Array.from(document.querySelectorAll('#process-list tr')).find(r => r.cells[0].textContent === '${pid}').querySelector('button').click()`);
    await waitFor(`!document.getElementById('inspector').hidden && document.getElementById('identity').textContent.includes('PID ${pid} /')`, 'process selection');
  }
  await choose(recursive.pid);
  await checkAutoSnapshot({evaluate, waitFor, delay, choose, otherPid: threads.pid, originalPid: recursive.pid});
  await checkProcessDetails({evaluate, waitFor, delay, choose, otherPid: threads.pid, originalPid: recursive.pid});
  await checkDescriptors({evaluate, waitFor, delay, choose, originalPid: recursive.pid, ipcPid: ipc.pid, peerPid});
  await waitFor("document.querySelectorAll('#maps tr').length > 5 && document.getElementById('connection').textContent.includes('Live')", 'SSE and memory maps');
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
  assert.equal(await evaluate("document.getElementById('auto-snapshot').checked"), false);
  // An open SSE connection must not hang graceful shutdown.
  app.kill('SIGTERM'); await until(() => app.exitCode !== null, 'shutdown with active SSE', 5000);
  const direct = launch('target/debug/procinsh', ['--listen', '127.0.0.1:0', '--pid', String(recursive.pid)]);
  const directUrl = await until(() => direct.output.match(/http:\/\/127\.0\.0\.1:\d+/)?.[0], 'direct PID server');
  await cdp('Page.navigate', {url: directUrl});
  await waitFor(`document.getElementById('identity')?.textContent.includes('PID ${recursive.pid} /')`, 'CLI direct PID');
  // Connection failures caused by deliberately shutting down the first server are expected.
  assert.deepEqual(errors.filter(e => !/ERR_CONNECTION_REFUSED|Failed to load resource/.test(e)), []);
  console.log('Browser checks passed: explorer, search, selection, SSE, snapshots, source lines, memory, mobile layout, thread switching, process exit, graceful shutdown, --pid.');
} finally {
  socket?.close();
  for (const child of children.reverse()) if (child.exitCode === null && child.signalCode === null) child.kill('SIGTERM');
  await delay(300);
  for (const child of children) if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
  await rm(profile, {recursive: true, force: true}).catch(() => {});
}
