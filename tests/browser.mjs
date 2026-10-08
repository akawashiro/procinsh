// Browser integration without npm dependencies. Requires Node >=22 and Chrome.
import {spawn} from 'node:child_process';
import {mkdtemp, rm, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import assert from 'node:assert/strict';
import {checkLiveSamples} from './live-samples.mjs';
import {checkProcessDetails} from './process-details.mjs';
import {checkDescriptors} from './fds.mjs';
import {checkProcessSessions} from './process-sessions.mjs';
import {processEventRecording} from './process-events.mjs';

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
  const app = launch('./scripts/dev_run.sh', ['--listen', '127.0.0.1:0']);
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
  const waitFor = (expression, label) => until(async () => {
    try { return await evaluate(expression); }
    catch (error) {
      if (/Execution context was destroyed|Cannot find context|Inspected target navigated/.test(String(error?.message || error))) return false;
      throw error;
    }
  }, label);
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
  await cdp('Page.addScriptToEvaluateOnNewDocument',{source:processEventRecording});
  await cdp('Emulation.setDeviceMetricsOverride', {width: 1440, height: 1100, deviceScaleFactor: 1, mobile: false});
  await cdp('Page.navigate', {url});
  await waitFor("document.querySelectorAll('#process-list tr').length > 2", 'process explorer');
  await waitFor("document.getElementById('error').hidden", 'initial SSE recovers from connection error');
  assert.equal(await evaluate("document.getElementById('inspector')"), null);
  assert.equal(await evaluate('window.targetSources.length'), 0, 'list opens no detail SSE');
  assert.equal(await evaluate('document.title'), 'procinsh');
  assert.equal(await evaluate("document.querySelector('header #back')"), null);
  const listReads = await evaluate("window.pageRequests.filter(p=>p==='/api/processes').length");
  await evaluate("window.dispatchEvent(new Event('pagehide'))");
  await delay(1200);
  assert.equal(await evaluate("window.pageRequests.filter(p=>p==='/api/processes').length"), listReads, 'pagehide stops list polling');
  await evaluate("window.dispatchEvent(new PageTransitionEvent('pageshow',{persisted:true}))");
  await waitFor(`window.pageRequests.filter(p=>p==='/api/processes').length>${listReads}`, 'list resumes after page cache');
  await evaluate("document.getElementById('sort').value='pid';document.getElementById('sort').dispatchEvent(new Event('change'))");
  assert.ok(await evaluate("(()=>{const pids=Array.from(document.querySelectorAll('#process-list tr'),r=>Number(r.cells[0].textContent));return pids.every((pid,i)=>!i||pids[i-1]<=pid)})()"), 'PID sort');
  async function choose(pid) {
    await waitFor("document.querySelectorAll('#process-list tr').length > 2", 'list ready');
    await evaluate(`document.getElementById('search').value = '${pid}'; document.getElementById('search').dispatchEvent(new Event('input'));`);
    await waitFor(`Array.from(document.querySelectorAll('#process-list tr')).some(r => r.cells[0].textContent === '${pid}')`, 'process search');
    const link = `Array.from(document.querySelectorAll('#process-list tr')).find(r => r.cells[0].textContent === '${pid}').querySelector('a')`;
    const href = await evaluate(`${link}.getAttribute('href')`);
    assert.match(href, new RegExp('^/process/'+pid+'\\?start_time_ticks=\\d+$'));
    await evaluate(`${link}.click()`);
    await waitFor(`document.getElementById('inspector')?.hidden===false && document.getElementById('identity')?.textContent.includes('PID ${pid} /')`, 'process selection');
    await waitFor("document.getElementById('error')?.hidden===true", 'initial SSE recovers');
    assert.equal(await evaluate("document.getElementById('explorer')"), null, 'detail has no list DOM');
    return href;
  }
  const recursiveHref = await choose(recursive.pid);
  const processName = await evaluate("document.getElementById('target-name').textContent");
  await evaluate("const reused=structuredClone(window.lastObservation);reused.summary.identity.start_time_ticks++;reused.summary.name='reused identity';window.targetSources.at(-1).dispatchEvent(new MessageEvent('observation',{data:JSON.stringify(reused)}))");
  assert.equal(await evaluate("document.getElementById('target-name').textContent"), processName, 'SSE cannot switch to a reused PID');
  const listLookups = await evaluate("window.pageRequests.filter(p=>p==='/api/processes').length");
  await delay(2200);
  assert.equal(await evaluate("window.pageRequests.filter(p=>p==='/api/processes').length"), listLookups, 'detail does not poll the list');
  const mismatchedHref = recursiveHref.replace(/start_time_ticks=(\d+)/, (_,ticks)=>'start_time_ticks='+(Number(ticks)+1));
  await cdp('Page.navigate',{url:url+mismatchedHref});
  await waitFor("document.getElementById('error')?.textContent.includes('PID was reused')", 'pinned identity mismatch');
  assert.equal(await evaluate('window.targetSources.length'), 0, 'mismatched identity opens no stream');
  assert.equal(await evaluate("document.getElementById('explorer')"), null);
  await cdp('Page.navigate',{url:url+'/process/'+recursive.pid+'?start_time_ticks=invalid'});
  await waitFor("document.getElementById('error')?.textContent.includes('Invalid process start time')", 'invalid start time');
  assert.equal(await evaluate('window.targetSources.length'), 0);
  await cdp('Page.navigate',{url:url+'/process/'+threads.pid});
  await waitFor(`document.getElementById('identity')?.textContent.includes('PID ${threads.pid} /')`,'direct URL selects requested target');
  await waitFor("window.targetSources.length===1",'direct URL opens one identified stream');
  await waitFor("document.getElementById('error')?.hidden===true", 'direct SSE connected');
  await evaluate("window.oldSource=window.targetSources[0];window.dispatchEvent(new Event('pagehide'))");
  await evaluate("window.oldSource.dispatchEvent(new MessageEvent('observation',{data:'invalid stale data'}))");
  assert.equal(await evaluate("document.getElementById('error').hidden"),true,'stopped stream events are ignored');
  assert.equal(await evaluate('window.oldSource.readyState'),2,'pagehide closes SSE');
  const pinnedIdentity = await evaluate('location.search');
  await evaluate("window.dispatchEvent(new PageTransitionEvent('pageshow',{persisted:true}))");
  await waitFor("window.targetSources.length===2 && document.getElementById('inspector')?.hidden===false && document.getElementById('error')?.hidden===true", 'page cache reconnect');
  assert.equal(await evaluate('location.search'), pinnedIdentity, 'page cache retains identity');
  const navigation = await cdp('Page.getNavigationHistory');
  const detailEntry = navigation.entries[navigation.currentIndex];
  await evaluate("window.oldSource.dispatchEvent(new MessageEvent('observation',{data:'invalid stale data'}));document.getElementById('back').click()");
  await waitFor("document.querySelectorAll('#process-list tr').length>2", 'back opens list');
  assert.equal(await evaluate('window.targetSources.length'),0);
  await cdp('Page.navigateToHistoryEntry',{entryId:detailEntry.id});
  await waitFor(`document.getElementById('identity')?.textContent.includes('PID ${threads.pid} /') && document.getElementById('error')?.hidden===true`, 'browser back restores details');
  assert.equal(await evaluate('location.search'), pinnedIdentity, 'browser history retains the observed identity');
  await evaluate("document.getElementById('back').click()");
  await choose(threads.pid);
  assert.ok(await evaluate(`document.getElementById('identity').textContent.includes('PID ${threads.pid} /')`),'stale initial stream is ignored');
  await cdp('Page.navigate',{url:url+'/process/2147483647'});
  await waitFor("document.getElementById('error')?.textContent.includes('Process exited')",'missing direct PID');
  assert.equal(await evaluate("document.getElementById('inspector').hidden"),true,'missing PID hides detail panels');
  assert.equal(await evaluate("document.getElementById('explorer')"),null,'missing PID stays on detail page');
  assert.equal(await evaluate("document.getElementById('back').getAttribute('href')"),'/list');
  await cdp('Page.navigate',{url:url+'/process/'+recursive.pid});
  await waitFor(`document.getElementById('identity')?.textContent.includes('PID ${recursive.pid} /')`,'direct URL original target');
  assert.equal(await evaluate("document.getElementById('target-status').hidden"), true);
  assert.equal(await evaluate("document.querySelector('header #back').hidden"), false);
  assert.equal(await evaluate("document.querySelector('header #back').textContent"), 'Go to list view');
  assert.equal(await evaluate('document.title'), 'procinsh');
  await checkProcessSessions({cdp,evaluate,choose,until,delay,url,debugPort,originalPid:recursive.pid,otherPid:threads.pid});
  await checkLiveSamples({evaluate, waitFor, delay, choose, otherPid: threads.pid, originalPid: recursive.pid});
  await checkProcessDetails({evaluate, waitFor, delay, choose, otherPid: threads.pid, originalPid: recursive.pid});
  await checkDescriptors({evaluate, waitFor, delay, choose, originalPid: recursive.pid, ipcPid: ipc.pid, peerPid});
  await waitFor("document.querySelectorAll('#maps tr').length > 5", 'memory maps');
  await waitFor("document.querySelectorAll('#registers tr').length === 18", 'live register sample');
  assert.match(await evaluate("document.getElementById('call-stack').textContent"), /foo/);
  assert.match(await evaluate("document.getElementById('call-stack').textContent"), /recursive\.c/);
  await waitFor("document.querySelectorAll('#disassembly tr').length > 0", 'disassembly from captured RIP');
  const ripText = () => evaluate("Array.from(document.querySelectorAll('#registers tr')).find(r => r.cells[0].textContent === 'RIP').cells[1].textContent");
  assert.equal(await evaluate("document.querySelector('#disassembly .current-instruction').cells[1].textContent"), await ripText());
  assert.match(await evaluate("document.querySelector('#disassembly tr').cells[2].textContent"), /^[0-9a-f]{2}( [0-9a-f]{2})*$/);
  assert.ok((await evaluate("document.querySelector('#disassembly tr').cells[3].textContent")).length > 0);
  assert.equal(await evaluate("document.querySelector('#memory-form, #memory-info, #memory')"), null, 'memory reader UI is removed');
  assert.equal(await evaluate("document.querySelector('#maps button, #registers button, #call-stack button, #disassembly button')"), null, 'addresses are displayed as text');
  assert.ok(await evaluate(`(() => {
    const grid = document.querySelector('.inspector-grid').getBoundingClientRect();
    const stack = document.getElementById('call-stack').closest('.panel').getBoundingClientRect();
    const name = document.getElementById('target-name').getBoundingClientRect();
    const controls = document.querySelector('.capture-heading').getBoundingClientRect();
    return Math.abs(stack.width - grid.width) < 1 && Math.abs(stack.left - grid.left) < 1 && controls.left >= name.right;
  })()`), 'stack spans full width and live status sits beside process name');
  const png = await cdp('Page.captureScreenshot', {format: 'png', captureBeyondViewport: true});
  await writeFile('target/browser-inspector.png', Buffer.from(png.data, 'base64'));
  await cdp('Emulation.setDeviceMetricsOverride', {width: 390, height: 844, deviceScaleFactor: 1, mobile: true});
  assert.equal(await evaluate('document.documentElement.scrollWidth <= 390'), true, 'mobile layout must not overflow');
  await evaluate("document.getElementById('back').click()");
  await waitFor("document.querySelectorAll('#process-list tr').length>2", 'return to explorer');
  assert.equal(await evaluate("document.getElementById('disassembly')"), null);
  await choose(threads.pid);
  await waitFor("document.querySelectorAll('#threads tr').length >= 6", 'thread view');

  await evaluate("document.querySelectorAll('#threads button')[1].click()");
  assert.match(await evaluate("document.getElementById('stack-tid').textContent"), /TID \d+/);
  await waitFor("document.querySelectorAll('#registers tr').length === 18", 'worker live sample');
  assert.equal(await evaluate("document.querySelector('#disassembly .current-instruction').cells[1].textContent"), await ripText());
  assert.equal(await evaluate("document.getElementById('error').hidden"), true);
  await evaluate("document.querySelector('#environment-panel summary').click()");
  await waitFor("document.getElementById('environment-info').textContent.includes('auto 5s')", 'environment before exit');
  threads.kill('SIGTERM');
  await waitFor("document.getElementById('target-status').textContent.includes('Process exited')", 'process exit');
  const detailReads = await evaluate("window.pageRequests.filter(p=>p.startsWith('/api/processes/environment?')).length");
  await delay(5500);
  assert.equal(await evaluate("window.pageRequests.filter(p=>p.startsWith('/api/processes/environment?')).length"), detailReads, 'process exit stops detail timer');
  assert.equal(await evaluate("document.getElementById('target-status').hidden"), false);
  assert.ok(await evaluate("window.targetSources.at(-1).readyState===EventSource.CLOSED"),'process exit closes the stream');
  await evaluate("document.getElementById('back').click()");
  await choose(recursive.pid);
  // An open SSE connection must not hang graceful shutdown.
  app.kill('SIGTERM'); await until(() => app.exitCode !== null, 'shutdown with active SSE', 5000);
  const removed = launch('./scripts/dev_run.sh', ['--pid', String(recursive.pid)]);
  await until(()=>removed.exitCode!==null,'removed CLI option');
  assert.notEqual(removed.exitCode,0);
  // Connection failures caused by deliberately shutting down the first server are expected.
  assert.deepEqual(errors.filter(e => !/ERR_CONNECTION_REFUSED|Failed to load resource/.test(e)), []);
  assert.deepEqual(targetGets,[],'UI never requests removed /api/target');
  console.log('Browser checks passed: explorer, search, selection, SSE initialization/direct URLs/stale events, live samples, source lines, removed memory UI, mobile layout, thread switching, process exit, graceful shutdown, removed --pid.');
} finally {
  socket?.close();
  for (const child of children.reverse()) if (child.exitCode === null && child.signalCode === null) child.kill('SIGTERM');
  await delay(300);
  for (const child of children) if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
  await rm(profile, {recursive: true, force: true}).catch(() => {});
}
