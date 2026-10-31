import type { ProcessChecks } from "./harness.js";
import assert from "node:assert/strict";
export async function checkLiveSamples({
  evaluate,
  waitFor,
  delay,
}: ProcessChecks) {
  assert.equal(
    await evaluate("document.querySelector('#snapshot, #auto-snapshot')"),
    null,
  );
  await waitFor(
    "[18,33,34].includes(document.querySelectorAll('#registers tr').length)",
    "automatic native registers",
  );
  assert.equal(
    await evaluate("document.getElementById('disasm-location')"),
    null,
  );
  assert.doesNotMatch(
    await evaluate<string>(
      "document.querySelector('.disassembly').textContent",
    ),
    /RIP marks the sampled instruction/,
  );
  await waitFor(
    "document.querySelectorAll('#disassembly tr').length > 0",
    "live disassembly instructions",
  );
  const tid = await evaluate(
    "Number(document.getElementById('stack-tid').textContent.match(/TID (\\d+)/)[1])",
  );
  const sample = `window.lastObservation.live_samples.find(t=>t.tid===${tid})`;
  const before = await evaluate(`${sample}.sampled_at`);
  await waitFor(
    `${sample}?.sampled_at > ${before}`,
    "selected thread live sample advances",
  );
  assert.equal(await evaluate("document.getElementById('sample-time')"), null);
  assert.ok(
    await evaluate(
      "window.lastObservation.live_samples.every(t=>!Object.hasOwn(t,'sample_source'))",
    ),
  );
  // Older samples retain their age in the thread list without a Stale label.
  await evaluate(`
    window.olderObservation = {...window.lastObservation, live_samples: window.lastObservation.live_samples.map(t=>({...t, sampled_at:Date.now()-10000, sample_age_ms:10000}))};
    window.targetSources.at(-1).dispatchEvent(new MessageEvent('observation',{data:JSON.stringify(window.olderObservation)}));
  `);
  const threadText = await evaluate<string>(
    "document.getElementById('threads').textContent",
  );
  assert.match(threadText, /10\.\ds/);
  assert.doesNotMatch(threadText, /Stale/);
  assert.equal(await evaluate("document.getElementById('sample-time')"), null);
  await delay(1200);
  await waitFor(
    `${sample}?.sample_age_ms < 10000`,
    "selected thread live samples continue updating",
  );

  // Captions, the current-instruction marker and register names come from the
  // snapshot, so AArch64 renders without ARM64 hardware. Dispatch and read in
  // one evaluation so live updates cannot interleave; dispatching also replaces
  // window.lastObservation, so keep the real native data.
  await evaluate("window.nativeObservation = window.lastObservation");
  const render = (observation: string) =>
    evaluate<{ caption: string; marker: string; registers: string[] }>(`(() => {
      window.targetSources.at(-1).dispatchEvent(new MessageEvent('observation', {data: JSON.stringify(${observation})}));
      return {
        caption: document.getElementById('disasm-time').textContent,
        marker: document.querySelector('#disassembly .current-instruction')?.cells[0].textContent ?? '',
        registers: Array.from(document.querySelectorAll('#registers tr'), r => r.cells[0].textContent),
      };
    })()`);
  const arm =
    await render(`{...window.nativeObservation, live_samples: window.nativeObservation.live_samples.map(t => t.tid !== ${tid} ? t : {
      ...t,
      architecture: 'aarch64',
      registers: [['PC','pc'],['SP','sp'],['X29','fp'],['X30','lr'],['PSTATE','flags'],['X0','general'],['X1','general']]
        .map(([name, role]) => ({...t.registers[0], name, role})),
    })}`);
  assert.equal(arm.caption, `TID ${tid} · Live best-effort · AArch64`);
  assert.equal(arm.marker, "→ PC");
  assert.deepEqual(arm.registers, [
    "PC",
    "SP",
    "X29",
    "X30",
    "PSTATE",
    "X0",
    "X1",
  ]);
  const native = await render("window.nativeObservation");
  const architecture = await evaluate<string>(`${sample}.architecture`);
  assert.equal(
    native.caption,
    `TID ${tid} · Live best-effort · ${architecture === "aarch64" ? "AArch64" : "x86-64 / Intel"}`,
  );
  assert.equal(native.marker, architecture === "aarch64" ? "→ PC" : "→ RIP");
  assert.ok(
    (architecture === "aarch64" ? [33, 34] : [18]).includes(
      native.registers.length,
    ),
  );
  assert.equal(native.registers[0], architecture === "aarch64" ? "PC" : "RIP");
  const unknown = await render(
    `{...window.nativeObservation, live_samples: window.nativeObservation.live_samples.map(t => ({...t, architecture: null}))}`,
  );
  assert.equal(
    unknown.caption,
    `TID ${tid} · Live best-effort`,
    "an unknown instruction set gets a neutral caption",
  );
  await render("window.nativeObservation");
}
