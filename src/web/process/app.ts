import { api, errorMessage } from "../shared/api.js";
import {
  Display,
  num,
  percent,
  bytes,
  rate,
  byteRate,
} from "../shared/display.js";
import { node, cell, button } from "../shared/dom.js";
import { same, query, processUrl } from "../shared/navigation.js";
import type {
  ProcessId,
  ProcessSummary,
  Target,
  ThreadSample,
  DetailData,
  FileDescriptors,
  DescriptorEndpoint,
} from "../shared/api-types.js";
import type { ProcessElements } from "./dom-types.js";

type DetailKind = keyof DetailData;

function $<K extends keyof ProcessElements>(id: K): ProcessElements[K] {
  const element = document.getElementById(id);
  if (!element) throw new Error(`Missing element: ${id}`);
  return element as ProcessElements[K];
}

let target: Target | null = null;
let selectedTid: number | null | undefined = null;
let liveSamples: ThreadSample[] = [];
let samplesReceivedAt = performance.now();
let mapsTimestamp: number | null = null;
const identity = () => target?.summary.identity;
let requestedId: ProcessId | null = null;
let active = true;
let startupGeneration = 0;
let detailEpoch = 0;
const detailRefreshMs = 5000;
let detailTimer: ReturnType<typeof setInterval> | null = null;
const detailKinds = ["environment", "auxv", "fds"] as const;
const processDetails: {
  [K in DetailKind]: { data: DetailData[K] | null; busy: boolean };
} = {
  environment: { data: null, busy: false },
  auxv: { data: null, busy: false },
  fds: { data: null, busy: false },
};
function stopDetailRefresh() {
  if (detailTimer !== null) clearInterval(detailTimer);
  detailTimer = null;
  detailEpoch++;
}
function updateDetailRefresh() {
  const open = detailKinds.some((kind) => $(`${kind}-panel`).open);
  if (!target || target.exited || !targetSource || !open) {
    if (detailTimer !== null) stopDetailRefresh();
    return;
  }
  if (detailTimer === null)
    detailTimer = setInterval(() => {
      for (const kind of detailKinds)
        if ($(`${kind}-panel`).open) loadProcessDetails(kind);
    }, detailRefreshMs);
}
function resetProcessDetails() {
  stopDetailRefresh();
  for (const kind of detailKinds) {
    processDetails[kind] = { data: null, busy: false };
    $(`${kind}-panel`).open = false;
    $(`${kind}-entries`).replaceChildren();
    $(`${kind}-error`).hidden = true;
    $(`${kind}-info`).textContent = "Not captured";
  }
  $("environment-search").value = "";
  $("fds-search").value = "";
  $("fds-warnings").hidden = true;
}
async function loadProcessDetails<K extends DetailKind>(kind: K) {
  const id = identity(),
    epoch = detailEpoch,
    view = processDetails[kind];
  if (!id || !target || target.exited || !targetSource || view.busy) return;
  view.busy = true;
  $(`${kind}-error`).hidden = true;
  $(`${kind}-info`).textContent = "Reading…";
  try {
    const data = await api<DetailData[K]>(
      `/api/processes/${kind}?${query(id)}`,
    );
    if (
      epoch !== detailEpoch ||
      !same(id, identity()) ||
      !same(id, data.process_id)
    )
      return;
    view.data = data;
    renderProcessDetails(kind);
  } catch (e) {
    if (epoch === detailEpoch && same(id, identity())) {
      renderProcessDetails(kind);
      $(`${kind}-error`).textContent =
        `${errorMessage(e)}${view.data ? " Showing the previous result." : ""}`;
      $(`${kind}-error`).hidden = false;
    }
  } finally {
    view.busy = false;
  }
}
function renderProcessDetails(kind: DetailKind) {
  const data = processDetails[kind].data;
  if (!data) {
    $(`${kind}-info`).textContent = "Not captured";
    return;
  }
  const time = new Date(data.captured_at).toLocaleTimeString("en-GB", {
    hour12: false,
  });
  if ("warnings" in data) {
    renderDescriptors(data, time);
    return;
  }
  if ("lossy_utf8" in data) {
    const search = $("environment-search").value.toLowerCase();
    const entries = data.entries.filter((e) =>
      `${e.name}=${e.value ?? ""}`.toLowerCase().includes(search),
    );
    $("environment-info").textContent =
      `${entries.length} / ${data.entries.length} entries · ${time} · auto 5s${data.lossy_utf8 ? " · Invalid UTF-8 is shown as �" : ""}`;
    $("environment-entries").replaceChildren(
      ...entries.map((e) => {
        const row = node("tr");
        cell(row, e.name, "mono");
        cell(row, e.value ?? "(no = sign)", "mono");
        return row;
      }),
    );
    if (!entries.length) {
      const row = node("tr");
      cell(
        row,
        data.entries.length
          ? "No matching environment variables."
          : "The environment is empty.",
        "muted",
      ).colSpan = 2;
      $("environment-entries").append(row);
    }
  } else {
    $("auxv-info").textContent =
      `${data.entries.length} entries · ELF${data.word_bits} · ${time} · auto 5s`;
    $("auxv-entries").replaceChildren(
      ...data.entries.map((e) => {
        const row = node("tr");
        cell(row, `${e.name} (${e.tag})`, "mono");
        cell(row, e.value, "mono");
        cell(row, e.decimal, "mono muted");
        const description = cell(row, e.description);
        if (e.text != null)
          description.append(node("div", e.text, "mono auxv-string"));
        if (e.text_error)
          description.append(
            node("div", `String: N/A · ${e.text_error}`, "muted"),
          );
        return row;
      }),
    );
  }
}
function renderDescriptors(data: FileDescriptors, time: string) {
  const search = $("fds-search").value.toLowerCase();
  const entries = data.entries.filter((e) =>
    `${e.fd} ${e.kind} ${Display.protocol(e.protocol)} ${Display.address(e.local) || e.path || ""} ${Display.address(e.remote)} ${e.target} ${[...e.peers, ...e.holders].map((p) => `${p.process_id.pid} ${p.name}`).join(" ")}`
      .toLowerCase()
      .includes(search),
  );
  $("fds-info").textContent =
    `${entries.length} / ${data.entries.length} FDs · ${time} · auto 5s`;
  $("fds-warnings").textContent = data.warnings.join(" ");
  $("fds-warnings").hidden = !data.warnings.length;
  $("fds-entries").replaceChildren(
    ...entries.map((e) => {
      const row = node("tr");
      row.dataset.fd = String(e.fd);
      cell(row, e.fd, "mono");
      cell(
        row,
        `${Display.protocol(e.protocol) || e.kind}\n${Display.access(e.access)}`,
        "mono",
      );
      const resource = cell(row, e.target, "mono muted");
      if (e.state) resource.append(node("div", Display.state(e.state)));
      if (e.path) resource.append(node("div", `Path: ${e.path}`));
      if (e.local)
        resource.append(node("div", `Local: ${Display.address(e.local)}`));
      if (e.remote)
        resource.append(node("div", `Remote: ${Display.address(e.remote)}`));
      if (e.peer_inode)
        resource.append(node("div", `Peer inode: ${e.peer_inode}`));
      const peers = cell(row);
      const endpoint = (p: DescriptorEndpoint, group: string) => {
        const div = node("div", null, "fd-endpoint");
        div.dataset.relation = group;
        const link = node(
          "a",
          `PID ${p.process_id.pid} · ${p.name}`,
          "pointer",
        );
        link.href = processUrl(p.process_id);
        link.dataset.pid = String(p.process_id.pid);
        div.append(
          link,
          node(
            "div",
            `FD ${p.fd} · ${Display.access(p.access)} · ${p.relation}`,
            "muted",
          ),
        );
        return div;
      };
      for (const p of e.peers) peers.append(endpoint(p, "peer"));
      if (e.note) peers.append(node("div", e.note, "muted"));
      if (e.holders.length) {
        const shared = node("details", null, "fd-holders");
        shared.append(
          node(
            "summary",
            `Holders of the same FD resource (${e.holders.length})`,
          ),
        );
        for (const p of e.holders) shared.append(endpoint(p, "holder"));
        peers.append(shared);
      }
      return row;
    }),
  );
  if (!entries.length) {
    const row = node("tr");
    cell(
      row,
      data.entries.length
        ? "No matching FDs."
        : "No observable pipes or sockets.",
      "muted",
    ).colSpan = 4;
    $("fds-entries").append(row);
  }
}

function error(e: unknown) {
  $("error").textContent = errorMessage(e);
  $("error").hidden = false;
}
function clearError() {
  $("error").hidden = true;
}
let targetSource: EventSource | null = null;
let targetGeneration = 0;
function closeTarget() {
  stopDetailRefresh();
  targetGeneration++;
  targetSource?.close();
  targetSource = null;
}
function connect(id: ProcessId) {
  closeTarget();
  target = null;
  resetSamples();
  $("inspector").hidden = true;
  $("loading").hidden = false;
  clearError();
  const generation = targetGeneration;
  const events = new EventSource(`/api/processes/events?${query(id)}`);
  targetSource = events;
  let disconnected = false;
  events.addEventListener("observation", (event) => {
    if (targetSource !== events || generation !== targetGeneration) return;
    try {
      const next = JSON.parse(event.data) as Target;
      if (!same(id, next.summary.identity)) return;
      if (disconnected) {
        clearError();
        disconnected = false;
      }
      acceptTarget(next);
      if (next.exited) {
        stopDetailRefresh();
        events.close();
        targetSource = null;
      }
    } catch (e) {
      error(e);
    }
  });
  events.onerror = () => {
    if (targetSource !== events || generation !== targetGeneration) return;
    disconnected = true;
    $("loading").hidden = true;
    error(
      new Error(
        "Process observation disconnected. Retrying the same process identity…",
      ),
    );
  };
}
function resetSamples() {
  resetProcessDetails();
  liveSamples = [];
  selectedTid = null;
  mapsTimestamp = null;
  $("registers").replaceChildren();
  $("call-stack").replaceChildren(node("p", "Waiting for sample", "muted"));
  $("disassembly").replaceChildren();
  $("disasm-error").hidden = true;
  $("disasm-time").textContent = "Live best-effort · x86-64 / Intel";
}
function acceptTarget(next: Target) {
  const changed = !same(identity(), next.summary.identity);
  if (changed) resetSamples();
  target = next;
  liveSamples = next.live_samples;
  samplesReceivedAt = performance.now();
  $("loading").hidden = true;
  $("inspector").hidden = false;
  renderTarget();
  renderLiveSample();
}

function formatStartTime(timestamp: number | null): string {
  if (timestamp === null) return "N/A";
  const date = new Date(timestamp);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}
function renderTarget() {
  if (!target) return;
  const p = target.summary,
    o = target.observation;
  $("target-name").textContent = p.name;
  $("identity").textContent =
    `PID ${p.identity.pid} / START ${formatStartTime(p.started_at)} / ${p.username ?? p.uid ?? "N/A"}`;
  $("command").textContent = p.command_line?.join(" ") || p.executable || "N/A";
  $("target-status").hidden = !target.exited;
  $("target-status").textContent = target.exited ? "● Process exited" : "";
  $("target-status").classList.toggle("exited", target.exited);
  $("target-error").hidden = !target.error;
  $("target-error").textContent = target.error || "";
  if (!o) return;
  const r = o.rates;
  const metrics = [
    [
      "CPU",
      percent(o.cpu_percent),
      `CPU ${o.cpu} · nice ${o.nice} · priority ${o.priority}`,
    ],
    ["RSS / VMS", bytes(o.rss_bytes), `VMS ${bytes(o.vms_bytes)}`],
    [
      "THREADS",
      num(o.threads.length, 0),
      `Last observation ${new Date(o.timestamp).toLocaleTimeString("en-GB", { hour12: false })}`,
    ],
    [
      "PAGE FAULTS",
      rate(r.minor_faults),
      `Major ${rate(r.major_faults)} · total ${num(o.minor_faults, 0)} / ${num(o.major_faults, 0)}`,
    ],
    [
      "CONTEXT SWITCHES",
      rate(r.voluntary_context_switches),
      `Nonvoluntary ${rate(r.nonvoluntary_context_switches)}`,
    ],
    [
      "I/O READ / WRITE",
      byteRate(r.read_bytes),
      `Write ${byteRate(r.write_bytes)} · totals ${bytes(o.io?.read_bytes)} / ${bytes(o.io?.write_bytes)}`,
    ],
  ];
  $("metrics").replaceChildren(
    ...metrics.map(([label, value, sub]) => {
      const div = node("div", null, "metric");
      div.append(
        node("div", label, "metric-label"),
        node("div", value, "metric-value"),
        node("div", sub, "metric-sub"),
      );
      return div;
    }),
  );
  const threads = o.threads;
  if (
    !threads.some((t) => t.tid === selectedTid) &&
    !liveSamples.some((t) => t.tid === selectedTid)
  ) {
    selectedTid = threads[0]?.tid;
    renderLiveSample();
  }
  $("thread-count").textContent = `${threads.length} threads`;
  $("threads").replaceChildren(
    ...threads.map((t) => {
      const row = node("tr", null, t.tid === selectedTid ? "selected" : "");
      cell(row).append(
        button(`${t.tid} ${t.name}`, () => {
          selectedTid = t.tid;
          renderTarget();
          renderLiveSample();
        }),
      );
      cell(row, percent(t.cpu_percent));
      cell(row, t.cpu);
      cell(row, t.state);
      const live = liveSamples.find((sample) => sample.tid === t.tid);
      const age = sampleAge(live);
      cell(
        row,
        live?.error ||
          (age == null ? "Waiting for sample" : `${(age / 1000).toFixed(1)}s`),
        "muted",
      );
      return row;
    }),
  );
  const thread = threads.find((t) => t.tid === selectedTid);
  $("thread-detail").textContent = thread
    ? `TID ${thread.tid} · ${Display.scheduler(thread.scheduler)} · priority ${thread.priority} · nice ${thread.nice} · affinity ${Display.affinity(thread.affinity)} · ctx ${num(thread.voluntary_context_switches, 0)} voluntary / ${num(thread.nonvoluntary_context_switches, 0)} involuntary`
    : "The selected thread has exited.";
  if (mapsTimestamp !== target.maps_captured_at || target.maps_error) {
    mapsTimestamp = target.maps_captured_at;
    $("maps-info").textContent =
      target.maps_error ||
      `${target.maps.length} mappings · PSS ${bytes(target.rollup?.pss_bytes)} · ${mapsTimestamp ? new Date(mapsTimestamp).toLocaleTimeString("en-GB", { hour12: false }) : "N/A"} · every 5s`;
    $("maps").replaceChildren(
      ...target.maps.map((m) => {
        const row = node("tr");
        const start = cell(row, null, "mono");
        start.append(node("span", m.start), node("div", m.end, "muted"));
        cell(row, Display.permissions(m), "mono");
        cell(row, bytes(m.rss_bytes));
        cell(row, bytes(m.pss_bytes));
        cell(row, m.file_offset, "mono");
        cell(row, m.pathname || "[anonymous]");
        return row;
      }),
    );
  }
  drawHistory();
}
function historyMemoryLimit(peak: number): number {
  return (
    [1024, 1024 ** 2, 1024 ** 3, 1024 ** 4].find((limit) => peak <= limit) ??
    1024 ** 4
  );
}
function drawHistory() {
  const canvas = $("history"),
    ctx = canvas.getContext("2d");
  if (!ctx || !target?.history.length) return;
  const width = Math.max(260, canvas.clientWidth - 32),
    height = 176,
    scale = window.devicePixelRatio || 1;
  canvas.width = width * scale;
  canvas.height = height * scale;
  ctx.scale(scale, scale);
  const points = target.history,
    end = points.at(-1)!.timestamp,
    rssMax = historyMemoryLimit(Math.max(...points.map((p) => p.rss_bytes))),
    left = 48,
    right = width - 76,
    top = 24,
    bottom = 148,
    plotWidth = right - left,
    plotHeight = bottom - top;
  ctx.font = "11px system-ui";
  ctx.textBaseline = "middle";
  ctx.fillStyle = "#66dfc5";
  ctx.textAlign = "right";
  ctx.fillText("CPU", left - 8, 9);
  ctx.fillStyle = "#8ab4ff";
  ctx.textAlign = "left";
  ctx.fillText("RSS", right + 8, 9);
  ctx.lineWidth = 1;
  for (let i = 0; i <= 4; i++) {
    const fraction = i / 4,
      y = bottom - fraction * plotHeight;
    ctx.strokeStyle = "#263246";
    ctx.beginPath();
    ctx.moveTo(left, y);
    ctx.lineTo(right, y);
    ctx.stroke();
    ctx.fillStyle = "#66dfc5";
    ctx.textAlign = "right";
    ctx.fillText(`${i * 25}%`, left - 8, y);
    ctx.fillStyle = "#8ab4ff";
    ctx.textAlign = "left";
    ctx.fillText(bytes(rssMax * fraction), right + 8, y);
    const x = left + fraction * plotWidth;
    ctx.strokeStyle = "#263246";
    ctx.beginPath();
    ctx.moveTo(x, top);
    ctx.lineTo(x, bottom);
    ctx.stroke();
    ctx.fillStyle = "#93a2b9";
    ctx.textAlign = "center";
    ctx.fillText(`${i * 15 - 60}s`, x, bottom + 18);
  }
  ctx.save();
  ctx.beginPath();
  ctx.rect(left, top, plotWidth, plotHeight);
  ctx.clip();
  for (const [key, max, color] of [
    ["cpu_percent", 100, "#66dfc5"],
    ["rss_bytes", rssMax, "#8ab4ff"],
  ] as const) {
    ctx.strokeStyle = color;
    ctx.lineWidth = 2;
    ctx.beginPath();
    let started = false;
    for (const p of points) {
      if (p[key] == null) {
        started = false;
        continue;
      }
      const x = left + (plotWidth * (p.timestamp - end + 60000)) / 60000,
        y = bottom - plotHeight * Math.min(1, Math.max(0, p[key] / max));
      if (started) ctx.lineTo(x, y);
      else ctx.moveTo(x, y);
      started = true;
    }
    ctx.stroke();
  }
  ctx.restore();
  $("history-scale").textContent = `CPU 0–100% · RSS 0–${bytes(rssMax)}`;
}

function sampleAge(thread: ThreadSample | undefined): number | null {
  return thread?.sample_age_ms == null
    ? null
    : thread.sample_age_ms + Math.max(0, performance.now() - samplesReceivedAt);
}
function renderDisassembly(thread: ThreadSample | undefined) {
  $("disassembly").replaceChildren();
  $("disasm-error").hidden = true;
  const code = thread?.disassembly;
  if (!thread || !code) return;
  if (code.error) {
    $("disasm-error").textContent = code.error;
    $("disasm-error").hidden = false;
  }
  $("disassembly").replaceChildren(
    ...code.instructions.map((instruction) => {
      const row = node(
        "tr",
        null,
        instruction.current ? "current-instruction" : "",
      );
      if (instruction.current) row.setAttribute("aria-current", "true");
      cell(row, instruction.current ? "→ RIP" : "", "mono");
      cell(row, instruction.address, "mono");
      cell(
        row,
        instruction.bytes.map((b) => b.toString(16).padStart(2, "0")).join(" "),
        "mono muted",
      );
      cell(row, instruction.text, "mono");
      return row;
    }),
  );
}
function renderLiveSample() {
  if (!target) return;
  $("disasm-time").textContent =
    `TID ${selectedTid} · Live best-effort · x86-64 / Intel`;
  const thread = liveSamples.find((t) => t.tid === selectedTid);
  renderDisassembly(thread);
  $("registers").replaceChildren();
  $("call-stack").replaceChildren();
  $("stack-tid").textContent = `TID ${selectedTid} · Frame pointer`;
  if (!thread) {
    $("call-stack").append(node("p", "Waiting for sample", "muted"));
    return;
  }
  for (const r of thread.registers) {
    const row = node("tr");
    cell(row, r.name, "mono");
    cell(row, r.value, "mono");
    cell(
      row,
      r.mapping
        ? `→ ${Display.mapping(r.mapping)} +${r.offset} (${r.kind.replaceAll("_", " ")})`
        : `→ ${r.decimal}`,
      "muted",
    );
    $("registers").append(row);
  }
  thread.call_stack.forEach((frame, i) => {
    const div = node("div", null, "frame");
    div.append(
      node("span", `#${i} `),
      node("span", frame.address),
      node(
        "span",
        ` ${frame.symbol || "??"}${frame.symbol_offset ? ` +${frame.symbol_offset}` : ""}`,
      ),
    );
    if (frame.source_file)
      div.append(node("small", `${frame.source_file}:${frame.line ?? "?"}`));
    for (const inline of frame.inline_frames.slice(1))
      div.append(
        node(
          "small",
          `↳ ${inline.function || "??"} ${inline.file || ""}:${inline.line ?? "?"}`,
        ),
      );
    $("call-stack").append(div);
  });
  $("call-stack").append(
    node("p", thread.error || thread.unwind_stop, "muted"),
  );
}
for (const kind of detailKinds) {
  $(`${kind}-panel`).addEventListener("toggle", () => {
    if ($(`${kind}-panel`).open) loadProcessDetails(kind);
    updateDetailRefresh();
  });
}
$("environment-search").addEventListener("input", () =>
  renderProcessDetails("environment"),
);
$("fds-search").addEventListener("input", () => renderProcessDetails("fds"));
addEventListener("pagehide", () => {
  active = false;
  startupGeneration++;
  closeTarget();
});
addEventListener("pageshow", (event) => {
  if (!event.persisted) return;
  active = true;
  if (target?.exited) return;
  if (requestedId) connect(requestedId);
  else start();
});
addEventListener("resize", drawHistory);

async function start() {
  const generation = ++startupGeneration;
  try {
    const match = /^\/process\/(\d+)$/.exec(location.pathname);
    const pid = Number(match?.[1]);
    if (!Number.isSafeInteger(pid) || pid <= 0)
      throw new Error("Invalid process PID");
    const ticks = new URLSearchParams(location.search).getAll(
      "start_time_ticks",
    );
    if (
      ticks.length > 1 ||
      (ticks.length &&
        (!/^\d+$/.test(ticks[0]) || !Number.isSafeInteger(Number(ticks[0]))))
    )
      throw new Error("Invalid process start time");
    const all = await api<ProcessSummary[]>("/api/processes");
    if (!active || generation !== startupGeneration) return;
    const process = all.find((p) => p.identity.pid === pid);
    if (
      !process ||
      (ticks.length && process.identity.start_time_ticks !== Number(ticks[0]))
    )
      throw new Error("Process exited or PID was reused");
    requestedId = process.identity;
    history.replaceState(null, "", processUrl(requestedId));
    connect(requestedId);
  } catch (e) {
    if (active && generation === startupGeneration) {
      $("loading").hidden = true;
      error(e);
    }
  }
}
start();
