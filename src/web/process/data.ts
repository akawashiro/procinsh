// Own process identity resolution, SSE observations, and additional panel reads.
import { api, errorMessage } from "../shared/api.js";
import { same, query } from "../shared/navigation.js";
import type {
  ProcessId,
  ProcessSummary,
  Target,
  ThreadSample,
  DetailData,
} from "../shared/api-types.js";

export type DetailKind = keyof DetailData;
export const detailKinds = ["environment", "auxv", "fds"] as const;
interface DetailView<D> {
  data: D | null;
  busy: boolean;
  error: string | null;
}
type DetailViews = { [K in DetailKind]: DetailView<DetailData[K]> };
export interface ProcessDataEvents {
  identity(id: ProcessId): void;
  reset(): void;
  snapshot(): void;
  detail(kind: DetailKind): void;
  loading(show: boolean): void;
  error(error: unknown | null): void;
}
const quietEvents: ProcessDataEvents = {
  identity() {},
  reset() {},
  snapshot() {},
  detail() {},
  loading() {},
  error() {},
};

export function processRequest(pathname: string, search: string) {
  const match = /^\/process\/(\d+)$/.exec(pathname);
  const pid = Number(match?.[1]);
  if (!Number.isSafeInteger(pid) || pid <= 0)
    throw new Error("Invalid process PID");
  const ticks = new URLSearchParams(search).getAll("start_time_ticks");
  if (
    ticks.length > 1 ||
    (ticks.length &&
      (!/^\d+$/.test(ticks[0]) || !Number.isSafeInteger(Number(ticks[0]))))
  )
    throw new Error("Invalid process start time");
  return { pid, startTime: ticks.length ? Number(ticks[0]) : null };
}

export class ProcessDataStore {
  target: Target | null = null;
  readonly details: DetailViews = {
    environment: { data: null, busy: false, error: null },
    auxv: { data: null, busy: false, error: null },
    fds: { data: null, busy: false, error: null },
  };
  private readonly openPanels = new Set<DetailKind>();
  private samplesReceivedAt = performance.now();
  private requestedId: ProcessId | null = null;
  private active = false;
  private startupGeneration = 0;
  private detailEpoch = 0;
  private detailTimer: ReturnType<typeof setInterval> | null = null;
  private source: EventSource | null = null;
  private sourceGeneration = 0;

  constructor(
    private readonly events: ProcessDataEvents = quietEvents,
    private readonly read = api,
    private readonly openEvents: (url: string) => EventSource = (url) =>
      new EventSource(url),
  ) {}

  private identity() {
    return this.target?.summary.identity;
  }
  sampleAge(thread: ThreadSample | undefined, now = performance.now()) {
    return thread?.sample_age_ms == null
      ? null
      : thread.sample_age_ms + Math.max(0, now - this.samplesReceivedAt);
  }
  private stopDetailRefresh() {
    if (this.detailTimer !== null) clearInterval(this.detailTimer);
    this.detailTimer = null;
    this.detailEpoch++;
  }
  private updateDetailRefresh() {
    if (
      !this.target ||
      this.target.exited ||
      !this.source ||
      !this.openPanels.size
    ) {
      if (this.detailTimer !== null) this.stopDetailRefresh();
      return;
    }
    if (this.detailTimer === null)
      this.detailTimer = setInterval(() => {
        for (const kind of this.openPanels) void this.loadDetails(kind);
      }, 5000);
  }
  setPanelOpen(kind: DetailKind, open: boolean) {
    if (open) {
      this.openPanels.add(kind);
      void this.loadDetails(kind);
    } else this.openPanels.delete(kind);
    this.updateDetailRefresh();
  }
  private resetDetails() {
    this.stopDetailRefresh();
    this.openPanels.clear();
    for (const kind of detailKinds)
      this.details[kind] = { data: null, busy: false, error: null };
    this.events.reset();
  }
  async loadDetails<K extends DetailKind>(kind: K) {
    const id = this.identity(),
      epoch = this.detailEpoch,
      view = this.details[kind];
    if (!id || !this.target || this.target.exited || !this.source || view.busy)
      return;
    view.busy = true;
    view.error = null;
    this.events.detail(kind);
    try {
      const data = await this.read<DetailData[K]>(
        `/api/processes/${kind}?${query(id)}`,
      );
      if (
        epoch !== this.detailEpoch ||
        !same(id, this.identity()) ||
        !same(id, data.process_id)
      )
        return;
      view.data = data;
      view.busy = false;
      this.events.detail(kind);
    } catch (e) {
      if (epoch === this.detailEpoch && same(id, this.identity())) {
        view.error = `${errorMessage(e)}${view.data ? " Showing the previous result." : ""}`;
        view.busy = false;
        this.events.detail(kind);
      }
    } finally {
      view.busy = false;
    }
  }
  private closeSource() {
    this.stopDetailRefresh();
    this.sourceGeneration++;
    this.source?.close();
    this.source = null;
  }
  private connect(id: ProcessId) {
    this.closeSource();
    this.target = null;
    this.resetDetails();
    this.events.loading(true);
    this.events.error(null);
    const generation = this.sourceGeneration;
    const events = this.openEvents(`/api/processes/events?${query(id)}`);
    this.source = events;
    let disconnected = false;
    events.addEventListener("observation", (event) => {
      if (this.source !== events || generation !== this.sourceGeneration)
        return;
      try {
        const next = JSON.parse(event.data) as Target;
        if (!same(id, next.summary.identity)) return;
        if (disconnected) {
          this.events.error(null);
          disconnected = false;
        }
        this.acceptTarget(next);
        if (next.exited) {
          this.stopDetailRefresh();
          events.close();
          this.source = null;
        }
      } catch (e) {
        this.events.error(e);
      }
    });
    events.onerror = () => {
      if (this.source !== events || generation !== this.sourceGeneration)
        return;
      disconnected = true;
      this.events.loading(false);
      this.events.error(
        new Error(
          "Process observation disconnected. Retrying the same process identity…",
        ),
      );
    };
  }
  private acceptTarget(next: Target) {
    if (!same(this.identity(), next.summary.identity)) this.resetDetails();
    this.target = next;
    this.samplesReceivedAt = performance.now();
    this.events.loading(false);
    this.events.snapshot();
  }
  stop() {
    this.active = false;
    this.startupGeneration++;
    this.closeSource();
  }
  resume(pathname: string, search: string) {
    this.active = true;
    if (this.target?.exited) return;
    if (this.requestedId) this.connect(this.requestedId);
    else void this.start(pathname, search);
  }
  async start(pathname: string, search: string) {
    this.active = true;
    const generation = ++this.startupGeneration;
    try {
      const { pid, startTime } = processRequest(pathname, search);
      const all = await this.read<ProcessSummary[]>("/api/processes");
      if (!this.active || generation !== this.startupGeneration) return;
      const process = all.find((p) => p.identity.pid === pid);
      if (
        !process ||
        (startTime !== null && process.identity.start_time_ticks !== startTime)
      )
        throw new Error("Process exited or PID was reused");
      this.requestedId = process.identity;
      this.events.identity(this.requestedId);
      this.connect(this.requestedId);
    } catch (e) {
      if (this.active && generation === this.startupGeneration) {
        this.events.loading(false);
        this.events.error(e);
      }
    }
  }
}
