import type { ProcessId } from "./api-types.js";

/** Link to the observed identity so navigation cannot select a reused PID. */
export function processUrl(id: ProcessId): string {
  const params = new URLSearchParams({
    start_time_ticks: String(id.start_time_ticks),
  });
  return `/process/${id.pid}?${params}`;
}

export const same = (
  a: ProcessId | null | undefined,
  b: ProcessId | null | undefined,
) => a && b && a.pid === b.pid && a.start_time_ticks === b.start_time_ticks;
export const query = (id: ProcessId) =>
  new URLSearchParams({
    pid: String(id.pid),
    start_time_ticks: String(id.start_time_ticks),
  }).toString();
