// Run after npm run build:web. Exercise the compiled chart with a recording canvas.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import {num, bytes} from '../dist/web/shared/display.js';

const source = readFileSync('dist/web/process/app.js', 'utf8');
const chart = source.slice(source.indexOf('function historyMemoryLimit('), source.indexOf('function sampleAge('));
const labels = [], paths = [];
const ctx = new Proxy({}, {
  get: (_, name) => (...args) => {
    if (name === 'fillText') labels.push(args);
    if (name === 'moveTo' || name === 'lineTo') paths.push([name, ...args]);
  },
});
const canvas = {clientWidth: 600, getContext: () => ctx};
const legend = {};
const scope = vm.createContext({
  num, bytes,
  window: {devicePixelRatio: 2},
  $: id => id === 'history' ? canvas : legend,
  target: {history: [
    {timestamp: 1000, cpu_percent: 0, rss_bytes: 1024 ** 2 / 2},
    {timestamp: 31000, cpu_percent: null, rss_bytes: 1024 ** 2},
    {timestamp: 61000, cpu_percent: 250, rss_bytes: 1024 ** 2},
  ]},
});
vm.runInContext(chart, scope);
for (const limit of [1024, 1024 ** 2, 1024 ** 3, 1024 ** 4]) {
  scope.peak = limit;
  assert.equal(vm.runInContext('historyMemoryLimit(peak)', scope), limit);
  scope.peak = limit + 1;
  assert.equal(vm.runInContext('historyMemoryLimit(peak)', scope), Math.min(limit * 1024, 1024 ** 4));
}
assert.equal(vm.runInContext('historyMemoryLimit(0)', scope), 1024);
vm.runInContext('drawHistory()', scope);
assert.equal(legend.textContent, 'CPU 0–100% · RSS 0–1 MiB');
for (const label of ['CPU', 'RSS', '0%', '25%', '50%', '75%', '100%', '0 B', '256 KiB', '512 KiB', '768 KiB', '1 MiB', '-60s', '-45s', '-30s', '-15s', '0s']) {
  assert.ok(labels.some(([text]) => text === label), `Missing axis label: ${label}`);
}
// CPU values above 100% stay at the top of the plot; null samples break the line.
assert.deepEqual(paths.slice(20, 22), [['moveTo', 48, 148], ['moveTo', 492, 24]]);
assert.equal(canvas.width, 1136);
assert.equal(canvas.height, 352);
console.log('History chart: scale boundaries, axes, CPU cap, sample gaps and pixel scaling passed.');
