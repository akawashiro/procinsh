// Architecture-dependent labels for live samples, derived from snapshot data.
import type { Architecture, ThreadSample } from "../shared/api-types.js";

const ARCHITECTURE_LABELS: Record<Architecture, string> = {
  x86_64: "x86-64 / Intel",
  aarch64: "AArch64",
};

/** Disassembly caption suffix; `null` while the instruction set is unknown. */
export function architectureLabel(
  architecture: Architecture | null | undefined,
): string | null {
  return architecture ? ARCHITECTURE_LABELS[architecture] : null;
}

/** Name of the register that marks the current instruction, such as RIP or PC. */
export function programCounterName(thread: ThreadSample): string {
  return (
    thread.registers.find((register) => register.role === "pc")?.name ??
    (thread.architecture === "x86_64" ? "RIP" : "PC")
  );
}

/** The process's instruction set, shared by all of its threads' samples. */
export function processArchitecture(
  samples: readonly ThreadSample[],
): Architecture | null {
  return samples.find((sample) => sample.architecture)?.architecture ?? null;
}

if (import.meta.vitest) {
  const { test } = import.meta.vitest;
  test("architecture labels and program counter by instruction set", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
    const register = (
      name: string,
      role: ThreadSample["registers"][number]["role"],
    ): ThreadSample["registers"][number] => ({
      name,
      role,
      value: "0x0000000000000000",
      decimal: "0",
      kind: "integer",
      mapping: null,
      offset: null,
    });
    const sample = (
      architecture: Architecture | null,
      registers: ThreadSample["registers"],
    ): ThreadSample => ({
      architecture,
      sampled_at: null,
      sample_age_ms: null,
      cpu: null,
      lost_samples: 0,
      tid: 1,
      error: null,
      unwind_stop: "",
      registers,
      call_stack: [],
      disassembly: null,
    });
    const x86 = sample("x86_64", [
      register("RIP", "pc"),
      register("RSP", "sp"),
      register("RBP", "fp"),
      register("RAX", "general"),
      register("RFLAGS", "flags"),
    ]);
    const arm = sample("aarch64", [
      register("PC", "pc"),
      register("SP", "sp"),
      register("X29", "fp"),
      register("X30", "lr"),
      register("X0", "general"),
      register("PSTATE", "flags"),
    ]);
    assert.equal(architectureLabel("x86_64"), "x86-64 / Intel");
    assert.equal(architectureLabel("aarch64"), "AArch64");
    assert.equal(architectureLabel(null), null);
    assert.equal(programCounterName(x86), "RIP");
    assert.equal(programCounterName(arm), "PC");
    assert.equal(
      programCounterName(sample("x86_64", [])),
      "RIP",
      "waiting x86-64 samples keep the RIP marker",
    );
    assert.equal(programCounterName(sample("aarch64", [])), "PC");
    assert.equal(processArchitecture([sample(null, []), arm]), "aarch64");
    assert.equal(processArchitecture([]), null);
  });
}
