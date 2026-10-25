// External-site workload using the same Puppeteer WebDriver BiDi API for both browsers.
import puppeteer from 'puppeteer-core';
import {mkdtemp, rm, writeFile, readFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';

const readSites = async path => (await readFile(path, 'utf8'))
  .split(/\r?\n/).map(line => line.trim()).filter(line => line && !line.startsWith('#'));
const urls = process.argv.slice(2);
if (process.env.SITE_LIST) urls.push(...await readSites(process.env.SITE_LIST));
else if (!urls.length) urls.push(...await readSites(new URL('../../scripts/browser_sites.txt', import.meta.url)));
if (!urls.length) throw new Error('The site list is empty');
for (const url of urls) {
  if (!['http:', 'https:'].includes(new URL(url).protocol)) throw new Error(`Invalid URL: ${url}`);
}
const tabCount = Number(process.env.TAB_COUNT ?? Math.min(8, urls.length));
if (!Number.isInteger(tabCount) || tabCount < 1 || tabCount > 1000) throw new Error('TAB_COUNT must be an integer between 1 and 1000');
const tabUrls = Array.from({length: tabCount}, (_, index) => urls[index % urls.length]);
const interval = Number(process.env.INTERVAL_SECONDS ?? 10) * 1000;
if (!Number.isFinite(interval) || interval < 1000) throw new Error('INTERVAL_SECONDS must be at least 1');
const browser = process.env.WORKLOAD ?? 'chromium';
if (!['chromium', 'firefox'].includes(browser)) throw new Error('WORKLOAD must be chromium or firefox');
const profile = await mkdtemp(join(tmpdir(), `${browser}-reload-profile-`));
const pidFile = process.env.PID_FILE;
let ownsPidFile = false;
let stopping = false;
let instance;
const log = message => console.log(`${new Date().toISOString()} ${message}`);
for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(signal, () => { stopping = true; });

try {
  if (pidFile) {
    await writeFile(pidFile, `${process.pid}\n`, {flag: 'wx'});
    ownsPidFile = true;
  }
  instance = await puppeteer.launch({
    // Puppeteer's Chrome launcher also controls Chromium.
    browser: browser === 'chromium' ? 'chrome' : 'firefox',
    protocol: 'webDriverBiDi',
    executablePath: browser === 'firefox'
      ? (process.env.FIREFOX || '/opt/firefox/firefox')
      : (process.env.CHROMIUM || '/usr/bin/chromium'),
    headless: !process.env.DISPLAY && !process.env.WAYLAND_DISPLAY,
    userDataDir: profile,
    handleSIGINT: false,
    handleSIGTERM: false,
    handleSIGHUP: false,
  });
  instance.on('disconnected', () => {
    if (!stopping) { console.error(`${browser} disconnected unexpectedly`); process.exitCode = 1; }
    stopping = true;
  });
  log(`controller PID=${process.pid}, ${browser} PID=${instance.process().pid}, interval=${interval / 1000}s, protocol=webDriverBiDi`);
  const initial = await instance.pages();
  const tabs = [];
  for (const url of tabUrls) {
    if (stopping) break;
    const page = await instance.newPage();
    await page.setCacheEnabled(false);
    page.on('load', () => log(`loaded ${page.url()}`));
    tabs.push({url, page});
    log(`opened tab ${tabs.length}/${tabCount} ${url}`);
  }
  await Promise.all(tabs.map(async ({url, page}) => {
    await page.goto(url, {waitUntil: 'domcontentloaded'});
  }));
  for (const page of initial) await page.close();
  log(`ready: ${tabs.length} page tabs`);
  let round = 0;
  let next = Date.now() + interval;
  while (!stopping) {
    await delay(Math.min(200, Math.max(0, next - Date.now())));
    if (stopping || Date.now() < next) continue;
    round++;
    await Promise.all(tabs.map(async ({url, page}) => {
      await page.reload({waitUntil: 'domcontentloaded'});
      log(`reload ${round} ${url}`);
    }));
    log(`reload round ${round} complete: ${tabs.length} tabs`);
    next += interval;
    if (next < Date.now()) next = Date.now() + interval;
  }
} catch (error) {
  if (!stopping) { console.error(error); process.exitCode = 1; }
} finally {
  stopping = true;
  try { await instance?.close(); }
  finally {
    await rm(profile, {recursive: true, force: true});
    if (ownsPidFile && (await readFile(pidFile, 'utf8').catch(() => '')).trim() === String(process.pid)) await rm(pidFile, {force: true});
  }
  log('stopped');
}
