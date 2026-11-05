import type { ProcessId } from "./api-types.js";

import { withHost } from "./hosts.js";

/**
 * Link to the observed identity so navigation cannot select a reused PID.
 * `id.host`, or else `host`, names a remote host.
 */
export function processUrl(id: ProcessId, host?: string | null): string {
  const params = new URLSearchParams({
    start_time_ticks: String(id.start_time_ticks),
  });
  return withHost(`/process/${id.pid}?${params}`, id.host ?? host);
}

export const same = (
  a: ProcessId | null | undefined,
  b: ProcessId | null | undefined,
) => a && b && a.pid === b.pid && a.start_time_ticks === b.start_time_ticks;
/** API query for a process; `id.host`, or else `host`, names a remote host. */
export const query = (id: ProcessId, host?: string | null) => {
  const params = new URLSearchParams({
    pid: String(id.pid),
    start_time_ticks: String(id.start_time_ticks),
  });
  const remote = id.host ?? host;
  if (remote) params.set("host", remote);
  return params.toString();
};
