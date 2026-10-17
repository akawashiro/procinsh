// Browser acquisition and transport controls; both loaders use the same parser.
import {
  parseRecording,
  SpaceRecorder,
  SpaceReplay,
  MAX_RECORDING_BYTES,
} from "./recording.js";
import type { RecordingEvent } from "./recording.js";
import type { SystemSnapshot } from "../shared/api-types.js";

export function mountRecordingControls(
  recorder: SpaceRecorder,
  snapshot: () => SystemSnapshot,
  ready: () => boolean,
) {
  const host = document.querySelector(".tools")!;
  const record = document.createElement("button");
  record.textContent = "Record";
  record.id = "record";
  const download = document.createElement("button");
  download.id = "download-recording";
  download.textContent = "Stop & Download";
  download.hidden = true;
  const status = document.createElement("span");
  status.setAttribute("role", "status");
  status.id = "recording-status";
  const open = document.createElement("button");
  open.id = "open-recording";
  open.type = "button";
  open.onclick = () => {
    location.assign(`${import.meta.env.BASE_URL}replay/space/`);
  };
  open.textContent = "Open recording";
  host.append(record, download, status, open);
  function refresh() {
    record.disabled = recorder.active || !!recorder.recording || !ready();
    download.hidden = !recorder.recording;
    download.textContent = recorder.active
      ? "Stop & Download"
      : "Download recording";
    if (recorder.recording)
      status.textContent = recorder.active
        ? `● REC ${(recorder.elapsed() / 1000).toFixed(1)}s`
        : `${recorder.recording.end_reason} (${(recorder.elapsed() / 1000).toFixed(1)}s saved)`;
  }
  record.onclick = () => {
    try {
      recorder.start(snapshot());
    } catch (error) {
      status.textContent = String(error);
    }
    refresh();
  };
  download.onclick = () => {
    recorder.stop();
    if (!recorder.recording) return;
    const url = URL.createObjectURL(
      new Blob([JSON.stringify(recorder.recording)], {
        type: "application/json;charset=utf-8",
      }),
    );
    const link = document.createElement("a");
    link.href = url;
    link.download = `procinsh-space-${recorder.recording.started_at.replace(/[:.]/g, "-")}.json`;
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
    recorder.recording = null;
    status.textContent = "Recording downloaded. Review before publishing.";
    refresh();
  };
  addEventListener("beforeunload", (event) => {
    if (recorder.recording) {
      event.preventDefault();
      event.returnValue = "";
    }
  });
  setInterval(refresh, 100);
  refresh();
}

export function mountReplayControls(
  reset: (snapshot: SystemSnapshot) => void,
  apply: (event: RecordingEvent) => void,
  clock: (time: number) => void,
) {
  const host = document.querySelector(".tools")!;
  const badge = document.createElement("strong");
  badge.textContent = "REPLAY";
  const file = document.createElement("input");
  file.type = "file";
  file.accept = ".json,application/json";
  file.setAttribute("aria-label", "Open recording JSON");
  const play = document.createElement("button");
  play.id = "play";
  const previous = document.createElement("button");
  previous.textContent = "Step backward";
  const next = document.createElement("button");
  next.textContent = "Step forward";
  const timeline = document.createElement("input");
  timeline.id = "timeline";
  timeline.type = "range";
  timeline.min = "0";
  timeline.max = "1";
  timeline.step = "any";
  timeline.setAttribute("aria-label", "Replay time");
  const speed = document.createElement("select");
  speed.setAttribute("aria-label", "Playback speed");
  for (const rate of [0.1, 0.25, 0.5, 1, 2]) {
    const option = document.createElement("option");
    option.value = String(rate);
    option.textContent = `${rate}×`;
    option.selected = rate === 1;
    speed.append(option);
  }
  const loop = document.createElement("input");
  loop.type = "checkbox";
  loop.id = "loop";
  const loopLabel = document.createElement("label");
  loopLabel.append(loop, " Loop");
  const status = document.createElement("span");
  status.id = "replay-status";
  status.setAttribute("role", "status");
  host.append(
    badge,
    file,
    previous,
    play,
    next,
    timeline,
    speed,
    loopLabel,
    status,
  );
  let replay: SpaceReplay | null = null;
  let loading = 0;
  let label = "";
  function refresh() {
    for (const element of [previous, play, next, timeline, speed, loop])
      element.disabled = !replay;
    play.textContent = replay?.playing ? "Pause" : "Play";
    timeline.disabled = !replay || replay.duration === 0;
    timeline.value = String(
      replay?.duration ? replay.time / replay.duration : 0,
    );
    timeline.setAttribute(
      "aria-valuetext",
      replay
        ? `${(replay.time / 1000).toFixed(1)} / ${(replay.duration / 1000).toFixed(1)} seconds`
        : "",
    );
    status.textContent = replay
      ? `${label} · ${(replay.time / 1000).toFixed(1)} / ${(replay.duration / 1000).toFixed(1)}s`
      : label;
  }
  async function load(read: () => Promise<string>, name: string) {
    const request = ++loading;
    if (replay) replay.playing = false;
    label = "Loading…";
    refresh();
    try {
      const text = await read();
      if (request !== loading) return;
      const recording = parseRecording(text);
      replay = new SpaceReplay(recording, reset, apply, clock);
      replay.speed = Number(speed.value);
      replay.loop = loop.checked;
      label = name;
    } catch (error) {
      if (request === loading) {
        replay = null;
        label = error instanceof Error ? error.message : String(error);
      }
    }
    refresh();
  }
  function local(recording: File | undefined) {
    if (!recording) return;
    void load(() => {
      if (recording.size > MAX_RECORDING_BYTES)
        throw new Error("Recording exceeds the 128 MiB local replay limit.");
      return recording.text();
    }, recording.name);
  }
  file.onchange = () => {
    local(file.files?.[0]);
    file.value = "";
  };
  document.addEventListener("dragover", (event) => {
    event.preventDefault();
  });
  document.addEventListener("drop", (event) => {
    event.preventDefault();
    local(event.dataTransfer?.files[0]);
  });
  play.onclick = () => {
    if (!replay) return;
    if (!replay.playing && replay.time === replay.duration) replay.seek(0);
    replay.playing = !replay.playing;
    refresh();
  };
  previous.onclick = () => {
    replay?.step(-1);
    refresh();
  };
  next.onclick = () => {
    replay?.step(1);
    refresh();
  };
  timeline.oninput = () => {
    if (replay) {
      replay.playing = false;
      replay.seek(Number(timeline.value) * replay.duration);
    }
    refresh();
  };
  speed.onchange = () => {
    if (replay) replay.speed = Number(speed.value);
  };
  loop.onchange = () => {
    if (replay) replay.loop = loop.checked;
  };
  document.addEventListener("visibilitychange", () => {
    if (document.hidden && replay) replay.playing = false;
    refresh();
  });
  const id = new URL(location.href).searchParams.get("recording");
  if (id) {
    const recordings: Record<string, string> = { demo: "recordings/demo.json" };
    if (Object.hasOwn(recordings, id))
      void load(async () => {
        const response = await fetch(
          `${import.meta.env.BASE_URL}${recordings[id]}`,
        );
        if (!response.ok)
          throw new Error(
            `Unable to load bundled recording (${response.status}).`,
          );
        return response.text();
      }, "Sanitized demo");
    else label = `Unknown bundled recording: ${id}. Choose a local JSON file.`;
  }
  refresh();
  let previousWallTime = performance.now();
  return () => {
    const wallTime = performance.now();
    replay?.tick(wallTime - previousWallTime);
    previousWallTime = wallTime;
    refresh();
  };
}
