import assert from 'node:assert/strict';
import {SpaceDataStore, key, fileKey} from '../dist/web/space/data.js';
import {visibleIds} from '../dist/web/space/search.js';

const a = {pid: 101, start_time_ticks: 1}, b = {pid: 102, start_time_ticks: 2};
const process = (identity, name, parent_id = null) => ({identity, name, parent_id, maps: []});
const resource = {kind: 'pipe', device: {major: 0, minor: 1}, inode: '7'};
const endpoint = process_id => ({process_id, fd: 3, resource});
const relation = {id: 'pipe', endpoint: endpoint(a), peer: endpoint(b), candidate: false, shared: false};
const initial = {processes: [process(a, 'writer'), process(b, 'reader', a)], fd_relations: [relation]};
const store = new SpaceDataStore();
store.replaceSnapshot(initial, false, 1000);
assert.deepEqual(Object.keys(store.nodes.get(key(a)).pos).sort(), ['x', 'y', 'z']);
const positions = [...store.nodes].map(([id, p]) => [id, {...p.pos}]);
store.replaceSnapshot({...initial, processes: [...initial.processes, process({pid: 103, start_time_ticks: 3}, 'new')]}, false, 1000);
for (const [id, pos] of positions) assert.deepEqual(store.nodes.get(id).pos, pos, 'structural additions preserve positions');
assert.deepEqual([...visibleIds(store.snapshot, ' WRITER ')], [key(a)]);
assert.deepEqual([...visibleIds(store.snapshot, '102')], [key(b)]);

const file = {process_id: a, file: {device: {major: 8, minor: 1}, inode: '9', generation: 0}, path: '/tmp/data.txt', bytes: 10, count: 1, write: true};
const update = store.ingestActivity({window_ms: 100, cpu: [{process_id: a, runtime_ns: 100, running_threads: 1}], files: [file], ipc: [{...endpoint(a), write: true, bytes: 4, count: 1}, {...endpoint(b), write: false, bytes: 4, count: 1}]}, new Set(store.nodes.keys()), 1000);
assert.equal(update.filesChanged, true);
assert.equal(update.routes.filter(r => r.kind === 'connection').length, 2);
assert.ok(update.routes.filter(r => r.kind === 'connection').every(r => r.direction === 1));
assert.deepEqual(store.edgeStats.get('pipe'), {bytes: 8, count: 2, time: 1000});
assert.equal(store.cpuGlows.get(key(a)).last, 1000);
assert.ok(store.filePositions.has(fileKey(file)));
const filtered = store.ingestActivity({ipc: [{...endpoint(a), write: true, bytes: 2, count: 1}]}, new Set([key(a)]), 1100);
assert.deepEqual(filtered.routes.map(r => r.kind), ['port'], 'a hidden peer cannot be used as an exact visible route');
assert.equal(store.edgeStats.get('pipe').time, 1000, 'filtered activity does not replace visible relation totals');
const ambiguous = {...relation, id: 'other', peer: endpoint({pid: 103, start_time_ticks: 3})};
store.replaceSnapshot({...store.snapshot, fd_relations: [relation, ambiguous]}, false, 1200);
assert.deepEqual(store.ingestActivity({ipc: [{...endpoint(a), write: true, bytes: 1, count: 1}]}, new Set(store.nodes.keys()), 1200).routes.map(r => r.kind), ['port'], 'multiple peers retain actor-only activity');
assert.equal(store.pruneFiles(31000), true, 'retained files expire at 30 seconds');
assert.equal(store.filePositions.size, 0);
store.replaceSnapshot({processes: [process({...a, start_time_ticks: 99}, 'reused')], fd_relations: []}, false, 32000);
assert.equal(store.cpuGlows.has(key(a)), false);
assert.equal(store.edgeStats.size, 0);
store.ingestActivity({files: [file], cpu: [{process_id: a, runtime_ns: 10}]}, new Set(store.nodes.keys()), 32000);
assert.equal(store.recentFiles.entries.size, 0, 'old process identities cannot create activity');
assert.equal(store.cpuGlows.has(key(a)), false);

class Source extends EventTarget {
  readyState = 0;
  constructor(url) { super(); this.url = url; }
  close() { this.readyState = 2; }
  emit(type, data) { this.dispatchEvent(new MessageEvent(type, {data: JSON.stringify(data)})); }
  open() { this.readyState = 1; this.onopen?.(new Event('open')); }
  error() { this.onerror?.(new Event('error')); }
}
const sources = [], seen = [];
const live = new SpaceDataStore({
  snapshot() { seen.push(['snapshot', live.snapshot.sequence]); },
  activity(update) { seen.push(['activity', update.routes.length]); },
  reset() { seen.push(['reset']); },
  gap() { seen.push(['gap']); },
  status(message) { seen.push(['status', message]); },
}, undefined, url => { const source = new Source(url); sources.push(source); return source; });
assert.equal(sources.length, 0, 'importing and constructing the data store starts no browser resources');
live.start(); live.start();
assert.equal(sources.length, 1, 'start is idempotent');
assert.equal(sources[0].url, '/api/system/events');
sources[0].open();
sources[0].emit('snapshot', {...initial, kind: 'full', sequence: 1});
assert.equal(live.snapshot.sequence, 1);
sources[0].emit('activity', {files: [file]});
assert.equal(live.recentFiles.entries.size, 1);
sources[0].emit('gap', {});
assert.equal(live.snapshot.sequence, 1, 'gap retains the structural baseline until the next full snapshot');
assert.ok(seen.some(([type]) => type === 'gap'));
sources[0].emit('snapshot', {kind: 'delta', sequence: 2, base_sequence: -1, processes: [], fd_relations: []});
assert.equal(sources[0].readyState, 2);
assert.equal(sources.length, 2, 'invalid baseline reconnects for a full snapshot');
assert.equal(live.recentFiles.entries.size, 0, 'reconnect clears transient activity');
sources[1].emit('snapshot', {...initial, kind: 'full', sequence: 1});
live.stop();
const afterStop = seen.length;
sources[1].emit('snapshot', {kind: 'full', sequence: 99, processes: [], fd_relations: []});
sources[1].error();
assert.equal(seen.length, afterStop, 'closed sources cannot update state or schedule retries');
live.start();
assert.equal(sources.length, 3);
sources[2].error();
assert.equal(sources[2].readyState, 2);
live.stop();
await new Promise(resolve => setTimeout(resolve, 3100));
assert.equal(sources.length, 3, 'stop cancels an error retry');
console.log('SPACE data passed: plain coordinates, stable layout, filtering, activity routes, identity reuse, expiry, lazy SSE, resynchronization, stale sources, retry cancellation.');
