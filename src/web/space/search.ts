// Filter visible process identities and handle search input.
import { key } from "./data.js";
import type { SystemSnapshot } from "../shared/api-types.js";
export function visibleIds(
  snapshot: SystemSnapshot,
  term: string,
): Set<string> {
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
