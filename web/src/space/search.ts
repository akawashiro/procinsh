// Filter visible process identities and handle search input.
import { key } from "./model.js";
import type { SystemSnapshot } from "../shared/api-types.js";
function visibleIds(snapshot: SystemSnapshot, term: string): Set<string> {
  const normalized = term.trim().toLowerCase();
  return new Set(
    snapshot.processes
      .filter(
        (n) =>
          !normalized ||
          `${n.identity.pid} ${n.name}`.toLowerCase().includes(normalized),
      )
      .map((n) => key(n.identity)),
  );
}
export function createSpaceSearch(
  input: HTMLInputElement,
  snapshot: () => SystemSnapshot,
  changed: () => void,
  choose: (id: string) => void,
) {
  const ids = () => visibleIds(snapshot(), input.value);
  const oninput = () => changed();
  const onkeydown = (event: KeyboardEvent) => {
    if (event.key === "Enter") {
      const first = ids().values().next().value;
      if (first) choose(first);
    }
  };
  input.addEventListener("input", oninput);
  input.addEventListener("keydown", onkeydown);
  return {
    visibleIds: ids,
    clear() {
      input.value = "";
    },
    dispose() {
      input.removeEventListener("input", oninput);
      input.removeEventListener("keydown", onkeydown);
    },
  };
}

if (import.meta.vitest) {
  const { test } = import.meta.vitest;
  test("visible process search", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
    const { processInfo } = await import("../../tests/support/fixtures.js");
    const a = { pid: 101, start_time_ticks: 1 },
      b = { pid: 102, start_time_ticks: 2 };
    const snapshot = {
      processes: [
        processInfo({ identity: a, name: "writer" }),
        processInfo({ identity: b, name: "reader" }),
      ],
      fd_relations: [],
    };
    assert.deepEqual([...visibleIds(snapshot, " WRITER ")], [key(a)]);
    assert.deepEqual([...visibleIds(snapshot, "102")], [key(b)]);
  });
}
