// Render process identity, status, metrics, and memory mappings.
import { errorMessage } from "../shared/api.js";
import {
  Display,
  num,
  percent,
  bytes,
  rate,
  byteRate,
} from "../shared/display.js";
import { node, cell } from "../shared/dom.js";
import type { ProcessDataStore } from "./data.js";
import { processElement as $ } from "./dom-types.js";
export function createProcessRenderer(data: ProcessDataStore) {
  let mapsTimestamp: number | null = null;
  function formatStartTime(timestamp: number | null): string {
    if (timestamp === null) return "N/A";
    const date = new Date(timestamp);
    const pad = (value: number) => String(value).padStart(2, "0");
    return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
  }
  function renderTarget() {
    const target = data.target;
    if (!target) return;
    $("inspector").hidden = false;
    const p = target.summary,
      o = target.observation;
    $("target-name").textContent = p.name;
    $("identity").textContent =
      `PID ${p.identity.pid} / START ${formatStartTime(p.started_at)} / ${p.username ?? p.uid ?? "N/A"}`;
    $("command").textContent =
      p.command_line?.join(" ") || p.executable || "N/A";
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
  }
  return {
    update: renderTarget,
    reset() {
      mapsTimestamp = null;
    },
    loading(show: boolean) {
      $("loading").hidden = !show;
      if (show) $("inspector").hidden = true;
    },
    error(error: unknown | null) {
      $("error").hidden = error === null;
      if (error !== null) $("error").textContent = errorMessage(error);
    },
  };
}
