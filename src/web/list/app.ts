import { api, errorMessage } from "../shared/api.js";
import { bytes, percent } from "../shared/display.js";
import { node, cell } from "../shared/dom.js";
import { processUrl } from "../shared/navigation.js";
import type { ProcessSummary } from "../shared/api-types.js";
import type { ListElements } from "./dom-types.js";

function $<K extends keyof ListElements>(id: K): ListElements[K] {
  const element = document.getElementById(id);
  if (!element) throw new Error(`Missing element: ${id}`);
  return element as ListElements[K];
}

let processes: ProcessSummary[] = [];
let active = false;
let generation = 0;
let request: AbortController | null = null;
let timer: ReturnType<typeof setInterval> | null = null;

function error(e: unknown) {
  $("error").textContent = errorMessage(e);
  $("error").hidden = false;
}

async function refresh() {
  if (!active || request) return;
  const current = new AbortController();
  request = current;
  try {
    const next = await api<ProcessSummary[]>("/api/processes", { signal: current.signal });
    if (!active || request !== current) return;
    processes = next;
    renderProcesses();
    $("error").hidden = true;
  } catch (e) {
    if (active && request === current) error(e);
  } finally {
    if (request === current) request = null;
  }
}

function renderProcesses() {
  const search = $("search").value.trim().toLowerCase();
  const rows = processes.filter((p) =>
    `${p.identity.pid} ${p.name} ${(p.command_line || []).join(" ")}`
      .toLowerCase()
      .includes(search),
  );
  rows.sort((a, b) =>
    $("sort").value === "pid"
      ? a.identity.pid - b.identity.pid
      : $("sort").value === "rss"
        ? b.rss_bytes - a.rss_bytes
        : (b.cpu_percent ?? -1) - (a.cpu_percent ?? -1),
  );
  const fragment = document.createDocumentFragment();
  for (const p of rows) {
    const row = node("tr");
    cell(row, p.identity.pid, "mono");
    cell(row, p.username ?? p.uid ?? "N/A");
    cell(row, percent(p.cpu_percent));
    cell(row, bytes(p.rss_bytes));
    cell(row, p.thread_count);
    cell(row, p.state);
    const detail = cell(row);
    const link = node("a", p.name, "process-link");
    link.href = processUrl(p.identity);
    detail.append(link);
    const command = node(
      "div",
      p.command_line?.join(" ") || p.executable || "N/A",
      "command-small",
    );
    command.title = command.textContent ?? "";
    detail.append(command);
    fragment.append(row);
  }
  if (!rows.length) {
    const row = node("tr");
    const td = cell(row, "No matching processes.", "muted");
    td.colSpan = 7;
    fragment.append(row);
  }
  $("process-list").replaceChildren(fragment);
  $("process-count").textContent =
    `${rows.length} / ${processes.length} processes`;
}
function stop() {
  active = false;
  generation++;
  if (timer !== null) clearInterval(timer);
  timer = null;
  request?.abort();
  request = null;
}

async function start() {
  stop();
  active = true;
  const current = generation;
  try {
    const config = await api<{ interval_ms: number }>("/api/config");
    if (!active || current !== generation) return;
    await refresh();
    if (active && current === generation)
      timer = setInterval(refresh, Math.max(1000, config.interval_ms));
  } catch (e) {
    if (active && current === generation) error(e);
  }
}

$("search").addEventListener("input", renderProcesses);
$("sort").addEventListener("change", renderProcesses);
addEventListener("pagehide", stop);
addEventListener("pageshow", (event) => {
  if (event.persisted) start();
});
start();
