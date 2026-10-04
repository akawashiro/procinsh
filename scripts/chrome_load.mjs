#!/usr/bin/env node
// Chrome load generator. Requires Node.js >=22 and Google Chrome.
// Usage: TAB_COUNT=20 INTERVAL_SECONDS=10 node scripts/chrome_load.mjs
import {spawn} from 'node:child_process';
import {mkdtemp, rm, writeFile, readFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';

const readSites = async path => (await readFile(path, 'utf8'))
  .split(/\r?\n/).map(line => line.trim()).filter(line => line && !line.startsWith('#'));
const urls = process.argv.slice(2);
if (process.env.SITE_LIST) urls.push(...await readSites(process.env.SITE_LIST));
else if (!urls.length) urls.push(...await readSites(new URL('./chrome_sites.txt', import.meta.url)));
if (!urls.length) throw new Error('The site list is empty');
for (const url of urls) {
  if (!['http:', 'https:'].includes(new URL(url).protocol)) throw new Error(`Invalid URL: ${url}`);
}
const tabCount = Number(process.env.TAB_COUNT ?? Math.min(20, urls.length));
if (!Number.isInteger(tabCount) || tabCount < 1 || tabCount > 1000) throw new Error('TAB_COUNT must be an integer between 1 and 1000');
const tabUrls = Array.from({length: tabCount}, (_, index) => urls[index % urls.length]);
const interval = Number(process.env.INTERVAL_SECONDS ?? 10) * 1000;
if (!Number.isFinite(interval) || interval < 1000) throw new Error('INTERVAL_SECONDS must be at least 1');
const profile = await mkdtemp(join(tmpdir(), 'chrome-reload-profile-'));
const sockets = [];
const pidFile = process.env.PID_FILE;
let ownsPidFile = false;
let stopping = false;
const log = message => console.log(`${new Date().toISOString()} ${message}`);
async function stopChild(child) {
  if (child.exitCode !== null || child.signalCode !== null || !child.pid) return;
  const exited = new Promise(resolve => child.once('exit', resolve));
  child.kill('SIGTERM');
  await Promise.race([exited, delay(3000)]);
  if (child.exitCode === null && child.signalCode === null) { child.kill('SIGKILL'); await exited; }
}
const chrome = spawn(process.env.CHROME || '/usr/bin/google-chrome', [
  ...(process.env.DISPLAY || process.env.WAYLAND_DISPLAY ? [] : ['--headless=new']),
  '--disable-gpu', '--disable-dev-shm-usage', '--no-first-run', '--no-default-browser-check', '--remote-debugging-address=127.0.0.1',
  '--remote-debugging-port=0', `--user-data-dir=${profile}`, 'about:blank',
], {stdio: ['ignore', 'ignore', 'pipe']});
let output = '';
chrome.stderr.on('data', data => { output = (output + data.toString()).slice(-20000); });
let launchError;
chrome.on('error', error => { launchError = error; });
let chromeExited = false;
chrome.on('exit', (code, signal) => {
  chromeExited = true;
  if (!stopping) {
    launchError = new Error(`Chrome exited unexpectedly (code=${code}, signal=${signal}): ${output}`);
    process.exitCode = 1;
  }
  stopping = true;
});
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => { stopping = true; });

async function connect(url) {
  const ws = new WebSocket(url);
  sockets.push(ws);
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Chrome connection timed out')), 15000);
    ws.onopen = () => { clearTimeout(timer); resolve(); };
    ws.onerror = () => { clearTimeout(timer); reject(new Error('Chrome connection failed')); };
  });
  let sequence = 0;
  const pending = new Map();
  ws.onmessage = event => {
    const message = JSON.parse(event.data);
    if (!message.id) return;
    const task = pending.get(message.id);
    if (!task) return;
    pending.delete(message.id);
    clearTimeout(task.timer);
    message.error ? task.reject(new Error(JSON.stringify(message.error))) : task.resolve(message.result);
  };
  ws.onclose = () => {
    for (const task of pending.values()) { clearTimeout(task.timer); task.reject(new Error('Chrome connection closed')); }
    pending.clear();
  };
  return (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++sequence;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`${method} timed out`)); }, 15000);
    pending.set(id, {resolve, reject, timer});
    ws.send(JSON.stringify({id, method, params}));
  });
}

try {
  if (pidFile) {
    await writeFile(pidFile, `${process.pid}\n`, {flag: 'wx'});
    ownsPidFile = true;
  }
  const deadline = Date.now() + 20000;
  let endpoint;
  while (!(endpoint = output.match(/ws:\/\/127\.0\.0\.1:\d+\/devtools\/browser\/[^\s]+/)?.[0])) {
    if (launchError || chromeExited || stopping || Date.now() > deadline) throw new Error(`Chrome failed to start: ${launchError || output}`);
    await delay(100);
  }
  log(`controller PID=${process.pid}, Chrome PID=${chrome.pid}, interval=${interval / 1000}s`);
  const cdp = await connect(endpoint);
  const initialTargets = (await cdp('Target.getTargets')).targetInfos.filter(target => target.type === 'page' && target.url === 'about:blank');
  const targets = [];
  for (const url of tabUrls) {
    if (stopping) break;
    const {targetId} = await cdp('Target.createTarget', {url: 'about:blank'});
    targets.push({url, targetId});
    log(`opened tab ${targets.length}/${tabCount} ${url}`);
  }
  const pages = await (await fetch(`http://127.0.0.1:${new URL(endpoint).port}/json/list`)).json();
  const tabs = [];
  const navigations = [];
  for (const {url, targetId} of targets) {
    if (stopping) break;
    const page = pages.find(page => page.id === targetId);
    if (!page) throw new Error(`Chrome tab disappeared: ${url}`);
    const pageCdp = await connect(page.webSocketDebuggerUrl);
    await pageCdp('Page.enable');
    await pageCdp('Network.enable');
    await pageCdp('Network.setCacheDisabled', {cacheDisabled: true});
    navigations.push(pageCdp('Page.navigate', {url}).then(result => {
      if (result.errorText) log(`navigation error ${url}: ${result.errorText}`);
    }).catch(error => log(`navigation error ${url}: ${error.message}`)));
    tabs.push({url, cdp: pageCdp});
  }
  await Promise.all(navigations);
  for (const target of initialTargets) await cdp('Target.closeTarget', {targetId: target.targetId});
  log(`ready: ${(await cdp('Target.getTargets')).targetInfos.filter(target => target.type === 'page').length} page tabs`);
  let round = 0;
  let next = Date.now() + interval;
  while (!stopping) {
    await delay(Math.min(200, Math.max(0, next - Date.now())));
    if (stopping || Date.now() < next) continue;
    round++;
    await Promise.all(tabs.map(async ({url, cdp: pageCdp}) => {
      await pageCdp('Page.reload', {ignoreCache: true});
      log(`reload ${round} ${url}`);
    }));
    log(`reload round ${round} complete: ${tabs.length} tabs`);
    next += interval;
    if (next < Date.now()) next = Date.now() + interval;
  }
} catch (error) {
  console.error(error);
  process.exitCode = 1;
} finally {
  stopping = true;
  for (const ws of sockets) ws.close();
  await stopChild(chrome);
  await rm(profile, {recursive: true, force: true});
  if (ownsPidFile && (await readFile(pidFile, 'utf8').catch(() => '')).trim() === String(process.pid)) await rm(pidFile, {force: true});
  log('stopped');
}
