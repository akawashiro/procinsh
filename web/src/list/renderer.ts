// Render process rows, identity-preserving links, counts, and request errors.
import { errorMessage } from "../shared/api.js";
import { bytes, percent } from "../shared/display.js";
import { node, cell } from "../shared/dom.js";
import { processUrl } from "../shared/navigation.js";
import type { ProcessSummary } from "../shared/api-types.js";
import { listElement as $ } from "./dom-types.js";
export function renderProcesses(rows: ProcessSummary[], total: number) {
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
  $("process-count").textContent = `${rows.length} / ${total} processes`;
}
export function renderError(error: unknown | null) {
  $("error").hidden = error === null;
  if (error !== null) $("error").textContent = errorMessage(error);
}
