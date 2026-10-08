// Render the selected thread's registers, call stack, and disassembly.
import { Display } from "../shared/display.js";
import { node, cell } from "../shared/dom.js";
import type { ThreadSample } from "../shared/api-types.js";
import type { ProcessDataStore } from "./data.js";
import type { ThreadSelection } from "./selection.js";
import { processElement as $ } from "./dom-types.js";
export function createProcessSamples(
  data: ProcessDataStore,
  selection: ThreadSelection,
) {
  function renderDisassembly(thread: ThreadSample | undefined) {
    $("disassembly").replaceChildren();
    $("disasm-error").hidden = true;
    const code = thread?.disassembly;
    if (!thread || !code) return;
    if (code.error) {
      $("disasm-error").textContent = code.error;
      $("disasm-error").hidden = false;
    }
    $("disassembly").replaceChildren(
      ...code.instructions.map((instruction) => {
        const row = node(
          "tr",
          null,
          instruction.current ? "current-instruction" : "",
        );
        if (instruction.current) row.setAttribute("aria-current", "true");
        cell(row, instruction.current ? "→ RIP" : "", "mono");
        cell(row, instruction.address, "mono");
        cell(
          row,
          instruction.bytes
            .map((b) => b.toString(16).padStart(2, "0"))
            .join(" "),
          "mono muted",
        );
        cell(row, instruction.text, "mono");
        return row;
      }),
    );
  }
  function renderLiveSample() {
    const target = data.target;
    if (!target) return;
    const selectedTid = selection.tid,
      liveSamples = target.live_samples;
    $("disasm-time").textContent =
      `TID ${selectedTid} · Live best-effort · x86-64 / Intel`;
    const thread = liveSamples.find((t) => t.tid === selectedTid);
    renderDisassembly(thread);
    $("registers").replaceChildren();
    $("call-stack").replaceChildren();
    $("stack-tid").textContent = `TID ${selectedTid} · Frame pointer`;
    if (!thread) {
      $("call-stack").append(node("p", "Waiting for sample", "muted"));
      return;
    }
    for (const r of thread.registers) {
      const row = node("tr");
      cell(row, r.name, "mono");
      cell(row, r.value, "mono");
      cell(
        row,
        r.mapping
          ? `→ ${Display.mapping(r.mapping)} +${r.offset} (${r.kind.replaceAll("_", " ")})`
          : `→ ${r.decimal}`,
        "muted",
      );
      $("registers").append(row);
    }
    thread.call_stack.forEach((frame, i) => {
      const div = node("div", null, "frame");
      div.append(
        node("span", `#${i} `),
        node("span", frame.address),
        node(
          "span",
          ` ${frame.symbol || "??"}${frame.symbol_offset ? ` +${frame.symbol_offset}` : ""}`,
        ),
      );
      if (frame.source_file)
        div.append(node("small", `${frame.source_file}:${frame.line ?? "?"}`));
      for (const inline of frame.inline_frames.slice(1))
        div.append(
          node(
            "small",
            `↳ ${inline.function || "??"} ${inline.file || ""}:${inline.line ?? "?"}`,
          ),
        );
      $("call-stack").append(div);
    });
    $("call-stack").append(
      node("p", thread.error || thread.unwind_stop, "muted"),
    );
  }
  return {
    update: renderLiveSample,
    reset() {
      $("registers").replaceChildren();
      $("call-stack").replaceChildren(node("p", "Waiting for sample", "muted"));
      $("disassembly").replaceChildren();
      $("disasm-error").hidden = true;
      $("disasm-time").textContent = "Live best-effort · x86-64 / Intel";
    },
  };
}
