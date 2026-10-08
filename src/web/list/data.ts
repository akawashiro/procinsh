// Own list requests, observations, polling, and cancellation across page visits.
import { api } from "../shared/api.js";
import type { ProcessSummary } from "../shared/api-types.js";

export interface ListDataEvents {
  changed(): void;
  error(error: unknown | null): void;
}
const quietEvents: ListDataEvents = { changed() {}, error() {} };

export class ListDataStore {
  processes: ProcessSummary[] = [];
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
      const next = await this.read<ProcessSummary[]>("/api/processes", {
        signal: current.signal,
      });
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
