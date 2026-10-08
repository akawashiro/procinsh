// Filter and order process observations, and handle list search controls.
import type { ProcessSummary } from "../shared/api-types.js";
export function matchingProcesses(
  processes: ProcessSummary[],
  term: string,
  sort: string,
) {
  const search = term.trim().toLowerCase();
  const rows = processes.filter((p) =>
    `${p.identity.pid} ${p.name} ${(p.command_line || []).join(" ")}`
      .toLowerCase()
      .includes(search),
  );
  rows.sort((a, b) =>
    sort === "pid"
      ? a.identity.pid - b.identity.pid
      : sort === "rss"
        ? b.rss_bytes - a.rss_bytes
        : (b.cpu_percent ?? -1) - (a.cpu_percent ?? -1),
  );
  return rows;
}
export function createListSearch(
  input: HTMLInputElement,
  sort: HTMLSelectElement,
  processes: () => ProcessSummary[],
  changed: () => void,
) {
  input.addEventListener("input", changed);
  sort.addEventListener("change", changed);
  return {
    rows: () => matchingProcesses(processes(), input.value, sort.value),
    dispose() {
      input.removeEventListener("input", changed);
      sort.removeEventListener("change", changed);
    },
  };
}
