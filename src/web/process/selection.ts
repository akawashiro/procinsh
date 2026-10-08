// Own thread selection and render the selectable thread table and its facts.
import { Display, num, percent } from "../shared/display.js";
import { node, cell, button } from "../shared/dom.js";
import type { Target } from "../shared/api-types.js";
import type { ProcessDataStore } from "./data.js";
import { processElement as $ } from "./dom-types.js";
export class ThreadSelection {
  tid: number | null | undefined = null;
  reset() {
    this.tid = null;
  }
  select(tid: number) {
    this.tid = tid;
  }
  retain(target: Target | null) {
    const threads = target?.observation?.threads;
    if (!threads) return;
    if (
      !threads.some((t) => t.tid === this.tid) &&
      !target.live_samples.some((t) => t.tid === this.tid)
    )
      this.tid = threads[0]?.tid;
  }
}
export function createThreadSelectionView(
  data: ProcessDataStore,
  selection: ThreadSelection,
  changed: () => void,
) {
  function render() {
    const target = data.target;
    if (!target?.observation) return;
    const threads = target.observation.threads,
      liveSamples = target.live_samples,
      selectedTid = selection.tid;
    $("thread-count").textContent = `${threads.length} threads`;
    $("threads").replaceChildren(
      ...threads.map((t) => {
        const row = node("tr", null, t.tid === selectedTid ? "selected" : "");
        cell(row).append(
          button(`${t.tid} ${t.name}`, () => {
            selection.select(t.tid);
            changed();
          }),
        );
        cell(row, percent(t.cpu_percent));
        cell(row, t.cpu);
        cell(row, t.state);
        const live = liveSamples.find((sample) => sample.tid === t.tid);
        const age = data.sampleAge(live);
        cell(
          row,
          live?.error ||
            (age == null
              ? "Waiting for sample"
              : `${(age / 1000).toFixed(1)}s`),
          "muted",
        );
        return row;
      }),
    );
    const thread = threads.find((t) => t.tid === selectedTid);
    $("thread-detail").textContent = thread
      ? `TID ${thread.tid} · ${Display.scheduler(thread.scheduler)} · priority ${thread.priority} · nice ${thread.nice} · affinity ${Display.affinity(thread.affinity)} · ctx ${num(thread.voluntary_context_switches, 0)} voluntary / ${num(thread.nonvoluntary_context_switches, 0)} involuntary`
      : "The selected thread has exited.";
  }
  return { update: render };
}
