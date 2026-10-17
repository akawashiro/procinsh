// Versioned, standalone JSON recordings shared by local and static loaders.
import type { SystemSnapshot, SpaceActivity } from "../shared/api-types.js";

export type RecordingEvent =
  | { at_ms: number; type: "snapshot"; data: SystemSnapshot }
  | { at_ms: number; type: "activity"; data: SpaceActivity };
export interface SpaceRecording {
  format_version: 1;
  view_type: "space";
  started_at: string;
  duration_ms: number;
  initial_snapshot: SystemSnapshot;
  events: RecordingEvent[];
  end_reason: string;
}
export const MAX_RECORDING_BYTES = 128 * 1024 * 1024;

type Check = (value: unknown, path: string) => void;
function invalid(path: string): never {
  throw new Error(`Invalid SPACE recording: ${path}`);
}
const number: Check = (v, p) => {
  if (typeof v !== "number" || !Number.isFinite(v) || v < 0) invalid(p);
};
const string: Check = (v, p) => {
  if (typeof v !== "string") invalid(p);
};
const boolean: Check = (v, p) => {
  if (typeof v !== "boolean") invalid(p);
};
const choice =
  (...values: unknown[]): Check =>
  (v, p) => {
    if (!values.includes(v)) invalid(p);
  };
const nullable =
  (check: Check): Check =>
  (v, p) => {
    if (v !== null) check(v, p);
  };
const optional =
  (check: Check): Check =>
  (v, p) => {
    if (v !== undefined) check(v, p);
  };
const array =
  (check: Check): Check =>
  (v, p) => {
    if (!Array.isArray(v)) invalid(p);
    v.forEach((item, i) => check(item, `${p}[${i}]`));
  };
const object =
  (fields: Record<string, Check>): Check =>
  (v, p) => {
    if (!v || typeof v !== "object" || Array.isArray(v)) invalid(p);
    for (const [key, check] of Object.entries(fields))
      check((v as Record<string, unknown>)[key], `${p}.${key}`);
  };
const identity = object({ pid: number, start_time_ticks: number });
const device = object({ major: number, minor: number });
const hex: Check = (v, p) => {
  if (typeof v !== "string" || !/^0x[\da-f]+$/i.test(v)) invalid(p);
};
const inode: Check = (v, p) => {
  if (typeof v !== "string" || !/^\d+$/.test(v)) invalid(p);
};
const memoryMap = object({
  start: hex,
  end: hex,
  file_offset: hex,
  device,
  inode,
  readable: boolean,
  writable: boolean,
  executable: boolean,
  private: boolean,
  pathname: nullable(string),
});
const process = object({
  identity,
  parent_id: nullable(identity),
  name: string,
  command_line: nullable(array(string)),
  uid: nullable(number),
  username: nullable(string),
  euid: nullable(number),
  effective_username: nullable(string),
  maps: array(memoryMap),
  maps_epoch: number,
  maps_error: nullable(string),
});
const resource = object({ kind: choice("pipe", "socket"), device, inode });
const endpoint = object({
  process_id: identity,
  fd: number,
  fd_count: number,
  resource,
  kind: choice("pipe", "socket", "fifo"),
  access: choice("read", "write", "read_write", "unknown"),
  connection_status: optional(
    nullable(choice("listening", "unconnected", "peer_present", "unknown")),
  ),
});
const address = nullable(object({ ip: string, port: number }));
const socketType = object({
  kind: choice(
    "stream",
    "dgram",
    "raw",
    "rdm",
    "seqpacket",
    "dccp",
    "packet",
    "unknown",
  ),
  code: optional(number),
});
const protocol: Check = (v, p) => {
  object({ kind: choice("tcp", "udp", "unix", "other") })(v, p);
  const kind = (v as { kind: string }).kind;
  if (kind === "tcp" || kind === "udp")
    object({ family: choice("ipv4", "ipv6") })(v, p);
  else if (kind === "unix") object({ socket_type: socketType })(v, p);
  else
    object({
      family: number,
      protocol: number,
      socket_type: nullable(socketType),
    })(v, p);
};
const snapshot = object({
  processes: array(process),
  sequence: optional(number),
  fd_relations: array(
    object({
      id: string,
      endpoint,
      peer: nullable(endpoint),
      label: string,
      candidate: boolean,
      shared: boolean,
      socket: nullable(
        object({
          protocol,
          state: nullable(object({ kind: string, code: optional(number) })),
          local: address,
          remote: address,
          path: nullable(string),
          network_peer: boolean,
          remote_hostname: nullable(string),
        }),
      ),
    }),
  ),
});
const sensor = object({
  state: choice("idle", "observing", "unavailable", "error"),
  message: optional(string),
});
const activity = object({
  status: object({ active: boolean, ipc: sensor, cpu: sensor, files: sensor }),
  window_ms: number,
  cpu: optional(
    array(
      object({
        process_id: identity,
        runtime_ns: number,
        switches: number,
        running_threads: number,
        cpus: array(number),
      }),
    ),
  ),
  ipc: optional(
    array(
      object({
        process_id: identity,
        resource,
        write: boolean,
        bytes: number,
        count: number,
      }),
    ),
  ),
  files: optional(
    array(
      object({
        process_id: identity,
        file: object({ device, inode, generation: number }),
        path: nullable(string),
        write: boolean,
        bytes: number,
        count: number,
      }),
    ),
  ),
  signals: optional(
    array(
      object({
        timestamp_ns: number,
        src_pid: number,
        dst_pid: number,
        signal: number,
        source_id: identity,
        destination_id: identity,
      }),
    ),
  ),
});

/** Unknown versions and malformed nested data are rejected before rendering. */
export function parseRecording(text: string): SpaceRecording {
  if (new Blob([text]).size > MAX_RECORDING_BYTES)
    throw new Error("Recording exceeds the 128 MiB local replay limit.");
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    throw new Error("Recording is not valid JSON.");
  }
  object({ format_version: number, view_type: string })(value, "recording");
  const recording = value as SpaceRecording;
  if (recording.format_version !== 1)
    throw new Error(
      `Unsupported recording format version: ${recording.format_version}`,
    );
  if (recording.view_type !== "space")
    throw new Error("This viewer requires a SPACE recording.");
  object({
    started_at: string,
    duration_ms: number,
    initial_snapshot: snapshot,
    events: array(
      object({ at_ms: number, type: choice("snapshot", "activity") }),
    ),
    end_reason: string,
  })(value, "recording");
  if (!Number.isFinite(Date.parse(recording.started_at))) invalid("started_at");
  let previous = 0;
  recording.events.forEach((event, i) => {
    if (event.at_ms < previous || event.at_ms > recording.duration_ms)
      invalid(`events[${i}].at_ms (must be ordered and within duration)`);
    (event.type === "snapshot" ? snapshot : activity)(
      event.data,
      `events[${i}].data`,
    );
    previous = event.at_ms;
  });
  return recording;
}

/** Keeps bounded data in memory; interruptions finish a recoverable download. */
export class SpaceRecorder {
  recording: SpaceRecording | null = null;
  active = false;
  private startTime = 0;
  private bytes = 0;
  constructor(private readonly now: () => number) {}
  start(initial: SystemSnapshot) {
    this.startTime = this.now();
    this.recording = {
      format_version: 1,
      view_type: "space",
      started_at: new Date().toISOString(),
      duration_ms: 0,
      initial_snapshot: structuredClone(initial),
      events: [],
      end_reason: "Recording",
    };
    this.bytes = new Blob([JSON.stringify(this.recording)]).size;
    if (this.bytes > MAX_RECORDING_BYTES) {
      this.recording = null;
      throw new Error("Initial snapshot exceeds the 128 MiB recording limit.");
    }
    this.active = true;
  }
  capture(
    event:
      | Omit<Extract<RecordingEvent, { type: "snapshot" }>, "at_ms">
      | Omit<Extract<RecordingEvent, { type: "activity" }>, "at_ms">,
  ) {
    if (!this.active || !this.recording) return;
    const next = { ...event, at_ms: this.elapsed() } as RecordingEvent;
    const size = new Blob([JSON.stringify(next)]).size + 1;
    if (this.bytes + size > MAX_RECORDING_BYTES - 4096) {
      this.stop("Size limit reached; download the retained recording.");
      return;
    }
    this.bytes += size;
    this.recording.events.push(structuredClone(next));
  }
  elapsed() {
    return this.active
      ? Math.max(0, this.now() - this.startTime)
      : (this.recording?.duration_ms ?? 0);
  }
  stop(reason = "Stopped by user") {
    if (this.active && this.recording) {
      this.recording.duration_ms = this.elapsed();
      this.recording.end_reason = reason;
    }
    this.active = false;
  }
}

/** Seeking rebuilds from the complete baseline, preserving event order. */
export class SpaceReplay {
  /** Last event time in milliseconds; a snapshot-only recording ends at zero. */
  readonly duration: number;
  time = 0;
  speed = 1;
  playing = false;
  loop = false;
  private cursor = 0;
  constructor(
    readonly recording: SpaceRecording,
    private readonly reset: (snapshot: SystemSnapshot) => void,
    private readonly apply: (event: RecordingEvent) => void,
    private readonly clock: (time: number) => void,
  ) {
    this.duration = recording.events.at(-1)?.at_ms ?? 0;
    this.seek(0);
  }
  seek(time: number) {
    this.time = 0;
    this.cursor = 0;
    this.clock(0);
    this.reset(this.recording.initial_snapshot);
    this.advanceTo(time);
  }
  private advanceTo(time: number) {
    this.time = Math.min(this.duration, Math.max(0, time));
    while (
      this.cursor < this.recording.events.length &&
      this.recording.events[this.cursor].at_ms <= this.time
    ) {
      const event = this.recording.events[this.cursor++];
      this.clock(event.at_ms);
      this.apply(event);
    }
    this.clock(this.time);
    if (this.time >= this.duration) this.playing = false;
  }
  tick(delta: number) {
    if (!this.playing) return;
    const target = this.time + Math.max(0, delta) * this.speed;
    const duration = this.duration;
    if (this.loop && duration > 0 && target >= duration) {
      this.advanceTo(duration);
      this.seek(target % duration);
      this.playing = true;
    } else {
      this.advanceTo(target);
    }
  }
  step(direction: -1 | 1) {
    this.playing = false;
    const times = [
      0,
      ...this.recording.events.map((e) => e.at_ms),
      this.duration,
    ];
    this.seek(
      direction > 0
        ? (times.find((t) => t > this.time) ?? this.duration)
        : ([...times].reverse().find((t) => t < this.time) ?? 0),
    );
  }
}
