'use strict';
const $ = id => document.getElementById(id);
const node = (tag, text, className) => { const e = document.createElement(tag); if (text != null) e.textContent = text; if (className) e.className = className; return e; };
const same = (a, b) => a && b && a.pid === b.pid && a.start_time_ticks === b.start_time_ticks;
const identity = () => target?.summary.identity;
const query = id => new URLSearchParams(id).toString();
const num = (v, digits = 1) => v == null ? 'N/A' : v.toLocaleString('en-US', {maximumFractionDigits: digits});
const percent = v => v == null ? 'N/A' : `${num(v)}%`;
function bytes(v) { if (v == null) return 'N/A'; const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB']; let i = 0; while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; } return `${num(v)} ${units[i]}`; }
const rate = v => v == null ? 'N/A' : `${num(v)}/s`;
const byteRate = v => v == null ? 'N/A' : `${bytes(v)}/s`;
let processes = [], target = null, selectedTid = null, captured = null, snapshotBusy = false, listBusy = false, selecting = false, mapsTimestamp = null;
let autoSnapshotTimer = null, snapshotEpoch = 0, autoSnapshotStatus = 'Auto capture OFF';
let detailEpoch = 0;
const detailKinds = ['environment', 'auxv', 'fds', 'signals'];
const processDetails = Object.fromEntries(detailKinds.map(kind => [kind, {data: null, busy: false}]));
function resetProcessDetails() {
  detailEpoch++;
  for (const kind of detailKinds) {
    processDetails[kind] = {data: null, busy: false};
    $(`${kind}-panel`).open = false;
    $(`${kind}-entries`).replaceChildren(); $(`${kind}-error`).hidden = true;
    $(`${kind}-info`).textContent = 'Not captured'; $(`${kind}-refresh`).disabled = false;
  }
  $('environment-search').value = '';
  $('fds-search').value = ''; $('fds-warnings').hidden = true;
}
async function loadProcessDetails(kind) {
  const id = identity(), epoch = detailEpoch, view = processDetails[kind];
  if (!id || target.exited || view.busy) return;
  view.busy = true; $(`${kind}-refresh`).disabled = true;
  $(`${kind}-error`).hidden = true; $(`${kind}-info`).textContent = 'Reading…';
  try {
    const data = await api(`/api/target/${kind}?${query(id)}`);
    if (epoch !== detailEpoch || !same(id, identity()) || !same(id, data.process_id)) return;
    view.data = data; renderProcessDetails(kind);
  } catch (e) {
    if (epoch === detailEpoch && same(id, identity())) {
      renderProcessDetails(kind);
      $(`${kind}-error`).textContent = `${e.message}${view.data ? ' Showing the previous result.' : ''}`;
      $(`${kind}-error`).hidden = false;
    }
  } finally {
    view.busy = false;
    if (epoch === detailEpoch) $(`${kind}-refresh`).disabled = !target || target.exited;
  }
}
function renderProcessDetails(kind) {
  const data = processDetails[kind].data;
  if (!data) { $(`${kind}-info`).textContent = 'Not captured'; return; }
  const time = new Date(data.captured_at).toLocaleTimeString('en-US');
  if (kind === 'signals') { renderSignals(data, time); return; }
  if (kind === 'fds') { renderDescriptors(data, time); return; }
  if (kind === 'environment') {
    const search = $('environment-search').value.toLowerCase();
    const entries = data.entries.filter(e => `${e.name}=${e.value ?? ''}`.toLowerCase().includes(search));
    $('environment-info').textContent = `${entries.length} / ${data.entries.length} entries · ${time}${data.lossy_utf8 ? ' · Invalid UTF-8 is shown as �' : ''}`;
    $('environment-entries').replaceChildren(...entries.map(e => { const row = node('tr'); cell(row, e.name, 'mono'); cell(row, e.value ?? '(no = sign)', 'mono'); return row; }));
    if (!entries.length) { const row = node('tr'); cell(row, data.entries.length ? 'No matching environment variables.' : 'The environment is empty.', 'muted').colSpan = 2; $('environment-entries').append(row); }
  } else {
    $('auxv-info').textContent = `${data.entries.length} entries · ELF${data.word_bits} · ${time}`;
    $('auxv-entries').replaceChildren(...data.entries.map(e => {
      const row = node('tr'); cell(row, `${e.name} (${e.tag})`, 'mono');
      const value = cell(row, null, 'mono');
      if (e.kind === 'address' && BigInt(e.value) !== 0n) value.append(button(e.value, () => readMemory(e.value)));
      else value.textContent = e.value;
      cell(row, e.decimal, 'mono muted');
      const description = cell(row, e.description);
      if (e.text != null) description.append(node('div', e.text, 'mono auxv-string'));
      if (e.text_error) description.append(node('div', `String: N/A · ${e.text_error}`, 'muted'));
      return row;
    }));
  }
}
function renderDescriptors(data, time) {
  const search = $('fds-search').value.toLowerCase();
  const entries = data.entries.filter(e => `${e.fd} ${e.kind} ${e.protocol || ''} ${e.local || ''} ${e.remote || ''} ${e.target} ${[...e.peers, ...e.holders].map(p => `${p.process_id.pid} ${p.name}`).join(' ')}`.toLowerCase().includes(search));
  $('fds-info').textContent = `${entries.length} / ${data.entries.length} FDs · ${time}`;
  $('fds-warnings').textContent = data.warnings.join(' '); $('fds-warnings').hidden = !data.warnings.length;
  $('fds-entries').replaceChildren(...entries.map(e => {
    const row = node('tr'); row.dataset.fd = e.fd;
    cell(row, e.fd, 'mono'); cell(row, `${e.protocol || e.kind}\n${e.access}`, 'mono');
    const resource = cell(row, e.target, 'mono muted');
    if (e.state) resource.append(node('div', e.state));
    if (e.local) resource.append(node('div', `Local: ${e.local}`));
    if (e.remote) resource.append(node('div', `Remote: ${e.remote}`));
    if (e.peer_inode) resource.append(node('div', `Peer inode: ${e.peer_inode}`));
    const peers = cell(row);
    const endpoint = (p, group) => {
      const div = node('div', null, 'fd-endpoint'); div.dataset.relation = group;
      const link = button(`PID ${p.process_id.pid} · ${p.name}`, () => select(p.process_id));
      link.dataset.pid = p.process_id.pid;
      div.append(link, node('div', `FD ${p.fd} · ${p.access} · ${p.relation}`, 'muted')); return div;
    };
    for (const p of e.peers) peers.append(endpoint(p, 'peer'));
    if (e.note) peers.append(node('div', e.note, 'muted'));
    if (e.holders.length) {
      const shared = node('details', null, 'fd-holders'); shared.append(node('summary', `Holders of the same FD resource (${e.holders.length})`));
      for (const p of e.holders) shared.append(endpoint(p, 'holder'));
      peers.append(shared);
    }
    return row;
  }));
  if (!entries.length) { const row = node('tr'); cell(row, data.entries.length ? 'No matching FDs.' : 'No observable pipes or sockets.', 'muted').colSpan = 4; $('fds-entries').append(row); }
}
function snapshotControls() {
  const enabled = autoSnapshotTimer !== null;
  $('snapshot').disabled = !target || target.exited || snapshotBusy || enabled;
  $('snapshot').textContent = snapshotBusy ? 'Capturing…' : 'Capture coherent snapshot';
  $('auto-snapshot').checked = enabled;
  $('auto-snapshot').disabled = !target || target.exited;
  $('auto-snapshot-status').textContent = `${autoSnapshotStatus}${snapshotBusy ? ' · Capturing' : ''}`;
}
function stopAutoSnapshot(reason = 'Auto capture OFF') {
  if (autoSnapshotTimer !== null) clearInterval(autoSnapshotTimer);
  autoSnapshotTimer = null; autoSnapshotStatus = reason; snapshotControls();
}
function startAutoSnapshot() {
  if (!target || target.exited || document.hidden || autoSnapshotTimer !== null) { snapshotControls(); return; }
  autoSnapshotStatus = 'Auto capture ON · every 1 s';
  autoSnapshotTimer = setInterval(() => { if (!document.hidden) snapshot(); else stopAutoSnapshot('Auto capture OFF · Tab hidden'); }, 1000);
  snapshotControls(); snapshot();
}
function error(e) { $('error').textContent = e?.message || String(e); $('error').hidden = false; }
function clearError() { $('error').hidden = true; }
async function api(path, options = {}) {
  const response = await fetch(path, {cache: 'no-store', ...options, headers: {'Content-Type': 'application/json', ...options.headers}});
  const body = await response.json().catch(() => ({error: `HTTP ${response.status}`}));
  if (!response.ok) throw new Error(body.error || `HTTP ${response.status}`);
  return body;
}
const cell = (row, text, className) => { const td = node('td', text, className); row.append(td); return td; };
function button(text, action, className = 'pointer') { const b = node('button', text, className); b.addEventListener('click', action); return b; }

async function refresh() {
  if (listBusy || !$('inspector').hidden) return;
  listBusy = true;
  try { processes = await api('/api/processes'); renderProcesses(); }
  catch (e) { error(e); }
  finally { listBusy = false; }
}
function renderProcesses() {
  const search = $('search').value.trim().toLowerCase();
  const rows = processes.filter(p => `${p.identity.pid} ${p.name} ${(p.command_line || []).join(' ')}`.toLowerCase().includes(search));
  rows.sort((a, b) => $('sort').value === 'pid' ? a.identity.pid - b.identity.pid : $('sort').value === 'rss' ? b.rss_bytes - a.rss_bytes : (b.cpu_percent ?? -1) - (a.cpu_percent ?? -1));
  const fragment = document.createDocumentFragment();
  for (const p of rows) {
    const row = node('tr');
    cell(row, p.identity.pid, 'mono'); cell(row, p.username ?? p.uid ?? 'N/A'); cell(row, percent(p.cpu_percent)); cell(row, bytes(p.rss_bytes)); cell(row, p.thread_count); cell(row, p.state);
    const detail = cell(row); detail.append(button(p.name, () => select(p.identity), 'process-link'));
    const command = node('div', p.command_line?.join(' ') || p.executable || 'N/A', 'command-small'); command.title = command.textContent; detail.append(command);
    fragment.append(row);
  }
  if (!rows.length) { const row = node('tr'); const td = cell(row, 'No matching processes.', 'muted'); td.colSpan = 7; fragment.append(row); }
  $('process-list').replaceChildren(fragment); $('process-count').textContent = `${rows.length} / ${processes.length} processes`;
}
async function select(id) {
  if (selecting) return;
  selecting = true; stopAutoSnapshot(); snapshotEpoch++; resetProcessDetails(); clearError();
  try { acceptTarget(await api('/api/target', {method: 'POST', body: JSON.stringify(id)})); }
  catch (e) { error(e); }
  finally { selecting = false; }
}
function resetCapture() {
  resetProcessDetails();
  stopAutoSnapshot(); snapshotEpoch++;
  captured = null; selectedTid = null; mapsTimestamp = null;
  $('registers').replaceChildren(); $('call-stack').replaceChildren(node('p', 'Capture a snapshot to view the call stack.', 'muted'));
  $('snapshot-time').textContent = 'Not captured · Capturing briefly pauses all threads.';
  $('disassembly').replaceChildren(); $('disasm-error').hidden = true;
  $('disasm-time').textContent = 'Snapshot · x86-64 / Intel';
  $('disasm-location').textContent = 'Capture a snapshot to view instructions starting at the selected thread’s RIP.';
  $('memory').textContent = ''; $('address').value = ''; $('memory-info').textContent = 'Click an address in a mapping or register to read memory.';
}
function acceptTarget(next) {
  if (!next) { target = null; resetCapture(); $('explorer').hidden = false; $('inspector').hidden = true; history.replaceState(null, '', '/'); document.title = 'procinsh / list'; refresh(); return; }
  if (!same(identity(), next.summary.identity)) resetCapture();
  target = next; $('explorer').hidden = true; $('inspector').hidden = false;
  if (target.exited && autoSnapshotTimer !== null) stopAutoSnapshot('Auto capture OFF · Process exited');
  history.replaceState(null, '', `/process/${identity().pid}`);
  document.title = `procinsh / ${target.summary.name}`;
  renderTarget();
}
async function back(event) {
  event?.preventDefault(); clearError(); const id = identity();
  stopAutoSnapshot(); snapshotEpoch++; resetProcessDetails();
  try { if (id) await api('/api/target', {method: 'DELETE', body: JSON.stringify(id)}); acceptTarget(null); }
  catch (e) { error(e); }
}
function renderTarget() {
  const p = target.summary, o = target.observation;
  $('target-name').textContent = p.name; $('identity').textContent = `PID ${p.identity.pid} / START ${p.identity.start_time_ticks} / ${p.username ?? p.uid ?? 'N/A'}`;
  $('command').textContent = p.command_line?.join(' ') || p.executable || 'N/A';
  $('target-status').textContent = target.exited ? '● Process exited' : '● Monitoring'; $('target-status').classList.toggle('exited', target.exited);
  $('target-error').hidden = !target.error; $('target-error').textContent = target.error || '';
  snapshotControls();
  for (const kind of detailKinds) $(`${kind}-refresh`).disabled = target.exited || processDetails[kind].busy;
  if (!o) return;
  const r = o.rates;
  const metrics = [
    ['CPU', percent(o.cpu_percent), `CPU ${o.cpu} · nice ${o.nice} · priority ${o.priority}`],
    ['RSS / VMS', bytes(o.rss_bytes), `VMS ${bytes(o.vms_bytes)}`],
    ['THREADS', num(o.threads.length, 0), `Last observation ${new Date(o.timestamp).toLocaleTimeString('en-US')}`],
    ['PAGE FAULTS', rate(r.minor_faults), `Major ${rate(r.major_faults)} · total ${num(o.minor_faults, 0)} / ${num(o.major_faults, 0)}`],
    ['CONTEXT SWITCHES', rate(r.voluntary_context_switches), `Nonvoluntary ${rate(r.nonvoluntary_context_switches)}`],
    ['I/O READ / WRITE', byteRate(r.read_bytes), `Write ${byteRate(r.write_bytes)} · totals ${bytes(o.io?.read_bytes)} / ${bytes(o.io?.write_bytes)}`]
  ];
  $('metrics').replaceChildren(...metrics.map(([label, value, sub]) => { const div = node('div', null, 'metric'); div.append(node('div', label, 'metric-label'), node('div', value, 'metric-value'), node('div', sub, 'metric-sub')); return div; }));
  const threads = o.threads;
  if (!threads.some(t => t.tid === selectedTid) && !captured?.threads.some(t => t.tid === selectedTid)) { selectedTid = threads[0]?.tid; renderSnapshot(); }
  $('thread-count').textContent = `${threads.length} threads`;
  $('threads').replaceChildren(...threads.map(t => {
    const row = node('tr', null, t.tid === selectedTid ? 'selected' : '');
    cell(row).append(button(`${t.tid} ${t.name}`, () => { selectedTid = t.tid; renderTarget(); renderSnapshot(); }));
    cell(row, percent(t.cpu_percent)); cell(row, t.cpu); cell(row, t.state); return row;
  }));
  const thread = threads.find(t => t.tid === selectedTid);
  $('thread-detail').textContent = thread ? `TID ${thread.tid} · ${thread.scheduler} · priority ${thread.priority} · nice ${thread.nice} · affinity ${thread.affinity ?? 'N/A'} · ctx ${num(thread.voluntary_context_switches, 0)} voluntary / ${num(thread.nonvoluntary_context_switches, 0)} involuntary` : 'The selected thread has exited.';
  if (mapsTimestamp !== target.maps_captured_at || target.maps_error) {
    mapsTimestamp = target.maps_captured_at;
    $('maps-info').textContent = target.maps_error || `${target.maps.length} mappings · PSS ${bytes(target.rollup?.pss_bytes)} · ${mapsTimestamp ? new Date(mapsTimestamp).toLocaleTimeString('en-US') : 'N/A'} · every 5s`;
    $('maps').replaceChildren(...target.maps.map(m => {
      const row = node('tr'); const start = cell(row, null, 'mono'); start.append(button(m.start, () => readMemory(m.start)), node('div', m.end, 'muted'));
      cell(row, m.permissions, 'mono'); cell(row, bytes(m.rss_bytes)); cell(row, bytes(m.pss_bytes)); cell(row, m.file_offset, 'mono'); cell(row, m.pathname || '[anonymous]'); return row;
    }));
  }
  drawHistory();
}
function drawHistory() {
  const canvas = $('history'), ctx = canvas.getContext('2d');
  if (!ctx || !target?.history.length) return;
  const width = Math.max(100, canvas.clientWidth - 32), height = 136, scale = window.devicePixelRatio || 1;
  canvas.width = width * scale; canvas.height = height * scale; ctx.scale(scale, scale);
  const points = target.history, end = points.at(-1).timestamp;
  const cpuMax = Math.max(100, ...points.map(p => p.cpu_percent ?? 0)), rssMax = Math.max(1, ...points.map(p => p.rss_bytes));
  ctx.strokeStyle = '#263246'; ctx.lineWidth = 1;
  for (let i = 0; i < 4; i++) { const y = 8 + i * 40; ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(width, y); ctx.stroke(); }
  for (const [key, max, color] of [['cpu_percent', cpuMax, '#66dfc5'], ['rss_bytes', rssMax, '#8ab4ff']]) {
    ctx.strokeStyle = color; ctx.lineWidth = 2; ctx.beginPath(); let started = false;
    for (const p of points) { if (p[key] == null) { started = false; continue; } const x = width * (p.timestamp - end + 60000) / 60000, y = 128 - 120 * p[key] / max; if (started) ctx.lineTo(x, y); else ctx.moveTo(x, y); started = true; }
    ctx.stroke();
  }
  $('history-scale').textContent = `CPU 0–${num(cpuMax)}% · RSS 0–${bytes(rssMax)}`;
}
async function snapshot() {
  const id = identity(), epoch = snapshotEpoch; if (!id || target.exited || snapshotBusy) return;
  clearError(); snapshotBusy = true; snapshotControls();
  try {
    const result = await api('/api/target/snapshot', {method: 'POST', body: JSON.stringify(id)});
    if (epoch === snapshotEpoch && same(id, identity()) && same(id, result.process_id)) { captured = result; renderTarget(); renderSnapshot(); }
  } catch (e) {
    if (epoch === snapshotEpoch && same(id, identity())) { stopAutoSnapshot('Auto capture OFF · Capture failed'); error(e); }
  } finally { snapshotBusy = false; snapshotControls(); }
}
function snapshotAge() {
  if (captured) $('snapshot-time').textContent = `Snapshot · ${Math.max(0, (Date.now() - captured.captured_at) / 1000).toFixed(1)}s ago · ${new Date(captured.captured_at).toLocaleTimeString('en-US')} · capture ${num(captured.paused_ms)}ms · ${captured.threads.length} threads`;
  if (captured) $('disasm-time').textContent = `TID ${selectedTid} · ${Math.max(0, (Date.now() - captured.captured_at) / 1000).toFixed(1)}s ago · ${new Date(captured.captured_at).toLocaleTimeString('en-US')} · x86-64 / Intel`;
}
function renderDisassembly(thread) {
  $('disassembly').replaceChildren(); $('disasm-error').hidden = true;
  const code = thread?.disassembly;
  if (!code) {
    $('disasm-location').textContent = thread?.error || 'This snapshot contains no instruction bytes for this thread.';
    return;
  }
  const rip = thread.registers.find(r => r.name === 'RIP');
  const frame = thread.call_stack[0];
  $('disasm-location').textContent = `RIP ${code.address} · ${rip?.mapping || 'mapping N/A'}${frame?.symbol ? ` · ${frame.symbol}${frame.symbol_offset ? ` +${frame.symbol_offset}` : ''}` : ''}${frame?.source_file ? ` · ${frame.source_file}:${frame.line ?? '?'}` : ''} · ${code.bytes.length} bytes captured`;
  if (code.error) { $('disasm-error').textContent = code.error; $('disasm-error').hidden = false; }
  $('disassembly').replaceChildren(...code.instructions.map(instruction => {
    const row = node('tr', null, instruction.current ? 'current-instruction' : '');
    if (instruction.current) row.setAttribute('aria-current', 'true');
    cell(row, instruction.current ? '→ RIP' : '', 'mono');
    const address = button(instruction.address, () => readMemory(instruction.address));
    address.title = 'Read current memory at this address (separate from the snapshot)';
    cell(row, null, 'mono').append(address);
    cell(row, instruction.bytes.map(b => b.toString(16).padStart(2, '0')).join(' '), 'mono muted');
    cell(row, instruction.text, 'mono'); return row;
  }));
}
function renderSnapshot() {
  if (!captured) return;
  snapshotAge();
  const thread = captured.threads.find(t => t.tid === selectedTid);
  renderDisassembly(thread);
  $('registers').replaceChildren(); $('call-stack').replaceChildren(); $('stack-tid').textContent = `TID ${selectedTid} · Frame pointer`;
  if (!thread) { $('call-stack').append(node('p', 'This thread is not included in the snapshot.', 'muted')); return; }
  for (const r of thread.registers) {
    const row = node('tr'); cell(row, r.name, 'mono'); const value = cell(row, null, 'mono');
    value.append(r.mapping ? button(r.value, () => readMemory(r.value)) : node('span', r.value));
    cell(row, r.mapping ? `→ ${r.mapping} +${r.offset} (${r.kind})` : `→ ${r.decimal}`, 'muted'); $('registers').append(row);
  }
  thread.call_stack.forEach((frame, i) => {
    const div = node('div', null, 'frame'); div.append(node('span', `#${i} `), button(frame.address, () => readMemory(frame.address)), node('span', ` ${frame.symbol || '??'}${frame.symbol_offset ? ` +${frame.symbol_offset}` : ''}`));
    if (frame.source_file) div.append(node('small', `${frame.source_file}:${frame.line ?? '?'}`));
    for (const inline of frame.inline_frames.slice(1)) div.append(node('small', `↳ ${inline.function || '??'} ${inline.file || ''}:${inline.line ?? '?'}`));
    $('call-stack').append(div);
  });
  $('call-stack').append(node('p', thread.error || thread.unwind_stop, 'muted'));
}
async function readMemory(address) {
  const id = identity(); if (!id) return; clearError(); $('address').value = address;
  const length = Number($('length').value);
  if (!Number.isInteger(length) || length < 1 || length > 65536) { error(new Error('Length must be 1–65536')); return; }
  try {
    const result = await api(`/api/target/memory?${query(id)}&address=${encodeURIComponent(address)}&length=${length}`);
    if (!same(id, identity())) return;
    const start = BigInt(result.address), lines = [];
    for (let i = 0; i < result.bytes.length; i += 16) {
      const chunk = result.bytes.slice(i, i + 16); const hex = chunk.map(b => b.toString(16).padStart(2, '0')).join(' ').padEnd(47, ' ');
      const ascii = chunk.map(b => b >= 32 && b <= 126 ? String.fromCharCode(b) : '.').join('');
      lines.push(`${(start + BigInt(i)).toString(16).padStart(16, '0')}  ${hex}  |${ascii}|`);
    }
    $('memory').textContent = lines.join('\n');
    $('memory-info').textContent = `${result.bytes.length} / ${result.requested_length} bytes${result.partial ? ' · partial read (mapping boundary)' : ''} · live read ${new Date(result.captured_at).toLocaleTimeString('en-US')} · separate from the snapshot`;
  } catch (e) { if (same(id, identity())) { $('memory').textContent = ''; $('memory-info').textContent = 'Read failed.'; error(e); } }
}
$('search').addEventListener('input', renderProcesses); $('sort').addEventListener('change', renderProcesses);
for (const kind of detailKinds) {
  $(`${kind}-panel`).addEventListener('toggle', () => { if ($(`${kind}-panel`).open && !processDetails[kind].data) loadProcessDetails(kind); });
  $(`${kind}-refresh`).addEventListener('click', () => loadProcessDetails(kind));
}
$('environment-search').addEventListener('input', () => renderProcessDetails('environment'));
$('fds-search').addEventListener('input', () => renderProcessDetails('fds'));
$('back').addEventListener('click', back); $('brand').addEventListener('click', back);
$('snapshot').addEventListener('click', () => { if (autoSnapshotTimer === null) snapshot(); });
$('auto-snapshot').addEventListener('change', () => { if ($('auto-snapshot').checked) startAutoSnapshot(); else stopAutoSnapshot(); });
document.addEventListener('visibilitychange', () => { if (document.hidden && autoSnapshotTimer !== null) stopAutoSnapshot('Auto capture OFF · Tab hidden'); });
window.addEventListener('pagehide', () => stopAutoSnapshot());
$('memory-form').addEventListener('submit', event => { event.preventDefault(); readMemory($('address').value.trim()); });
window.addEventListener('resize', drawHistory);
async function start() {
  try {
    const config = await api('/api/config'), initial = await api('/api/target');
    const direct = /^\/process\/(\d+)$/.exec(location.pathname);
    if (direct && Number(direct[1]) !== initial?.summary.identity.pid) {
      const all = await api('/api/processes'); const p = all.find(p => p.identity.pid === Number(direct[1]));
      if (p) await select(p.identity); else { acceptTarget(null); error(new Error('Process exited')); }
    } else acceptTarget(initial);
    const events = new EventSource('/api/target/events');
    events.addEventListener('observation', event => { try { acceptTarget(JSON.parse(event.data)); } catch (e) { error(e); } });
    events.onerror = () => { if (autoSnapshotTimer !== null) stopAutoSnapshot('Auto capture OFF · Disconnected'); };
    setInterval(refresh, Math.max(1000, config.interval_ms)); setInterval(snapshotAge, 1000);
  } catch (e) { error(e); }
}
start();

function renderSignals(data, time) {
  $('signals-info').textContent = `${data.threads.length} threads · ${time} · SigQ ${data.leader.queued} (queued for real UID / target limit)`;
  const rows = [];
  const add = (label, mask) => {
    const row = node('tr'); cell(row, label);
    cell(row, mask.hex, 'mono'); cell(row, mask.signals.join(', ') || 'None', 'mono'); rows.push(row);
  };
  add('Process-shared pending · ShdPnd', data.leader.shared_pending);
  add('Ignored · SigIgn', data.leader.ignored);
  add('Handler registered · SigCgt', data.leader.caught);
  for (const thread of data.threads) {
    add(`TID ${thread.tid} ${thread.name} · Pending SigPnd`, thread.pending);
    add(`TID ${thread.tid} ${thread.name} · Blocked SigBlk`, thread.blocked);
  }
  for (const warning of data.warnings) { const row = node('tr'); cell(row, warning, 'notice').colSpan = 3; rows.push(row); }
  $('signals-entries').replaceChildren(...rows);
}
