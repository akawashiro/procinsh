// Filter and order process observations, and handle list search controls.
import type { ProcessSummary } from "../shared/api-types.js";
function matchingProcesses(
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

if (import.meta.vitest) {
  const { test } = import.meta.vitest;
  test("process filtering and ordering", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
    const { processSummary } = await import("../../tests/support/fixtures.js");
    const processes = [
      processSummary({
        identity: { pid: 10, start_time_ticks: 100 },
        name: "main",
        cpu_percent: null,
        rss_bytes: 5,
        command_line: ["--worker"],
      }),
      processSummary({
        identity: { pid: 11, start_time_ticks: 100 },
        name: "worker",
        cpu_percent: 30,
        rss_bytes: 10,
        command_line: ["--worker"],
      }),
      processSummary({
        identity: { pid: 12, start_time_ticks: 100 },
        name: "other",
        cpu_percent: 10,
        rss_bytes: 20,
        command_line: ["--worker"],
      }),
    ];
    assert.deepEqual(
      matchingProcesses(processes, " --WORKER ", "pid").map(
        (p) => p.identity.pid,
      ),
      [10, 11, 12],
    );
    assert.deepEqual(
      matchingProcesses(processes, "WORK", "rss").map((p) => p.identity.pid),
      [12, 11, 10],
    );
    assert.deepEqual(
      matchingProcesses(processes, "", "cpu").map((p) => p.identity.pid),
      [11, 12, 10],
    );
    assert.deepEqual(
      processes.map((p) => p.identity.pid),
      [10, 11, 12],
      "sorting does not reorder stored data",
    );
  });
}
