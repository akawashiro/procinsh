// Own list requests, observations, polling, and cancellation across page visits.

import { api } from "../shared/api.js";
import type { ProcessSummary } from "../shared/api-types.js";
import { withHost } from "../shared/hosts.js";

export interface ListDataEvents {
  changed(): void;
  error(error: unknown | null): void;
}
const quietEvents: ListDataEvents = { changed() {}, error() {} };

export class ListDataStore {
  processes: ProcessSummary[] = [];
  /** The remote host whose processes are listed; null lists this machine. */
  host: string | null = null;
  private active = false;
  private generation = 0;
  private request: AbortController | null = null;
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor(
    private readonly events: ListDataEvents = quietEvents,
    private readonly read = api,
  ) {}

  async refresh() {
    if (!this.active || this.request) return;
    const current = new AbortController();
    this.request = current;
    try {
      const next = await this.read<ProcessSummary[]>(
        withHost("/api/processes", this.host),
        { signal: current.signal },
      );
      if (!this.active || this.request !== current) return;
      this.processes = next;
      this.events.changed();
      this.events.error(null);
    } catch (e) {
      if (this.active && this.request === current) this.events.error(e);
    } finally {
      if (this.request === current) this.request = null;
    }
  }
  stop() {
    this.active = false;
    this.generation++;
    if (this.timer !== null) clearInterval(this.timer);
    this.timer = null;
    this.request?.abort();
    this.request = null;
  }
  /** Lists `host` from now on, without the previous host's processes. */
  setHost(host: string | null) {
    if (host === this.host) return;
    this.host = host;
    this.processes = [];
    this.events.changed();
    if (this.active) void this.start();
  }
  async start() {
    this.stop();
    this.active = true;
    const current = this.generation;
    try {
      const config = await this.read<{ interval_ms: number }>("/api/config");
      if (!this.active || current !== this.generation) return;
      await this.refresh();
      if (this.active && current === this.generation)
        this.timer = setInterval(
          () => this.refresh(),
          Math.max(1000, config.interval_ms),
        );
    } catch (e) {
      if (this.active && current === this.generation) this.events.error(e);
    }
  }
}
