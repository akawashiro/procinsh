// Filter retained environment/FD data and handle additional-panel search input.
import { Display } from "../shared/display.js";
import type { Environment, FileDescriptors } from "../shared/api-types.js";
import type { DetailKind } from "./data.js";
import { processElement as $ } from "./dom-types.js";
export function matchingEnvironment(
  entries: Environment["entries"],
  term: string,
) {
  const search = term.toLowerCase();
  return entries.filter((e) =>
    `${e.name}=${e.value ?? ""}`.toLowerCase().includes(search),
  );
}
export function matchingDescriptors(
  entries: FileDescriptors["entries"],
  term: string,
) {
  const search = term.toLowerCase();
  return entries.filter((e) =>
    `${e.fd} ${e.kind} ${Display.protocol(e.protocol)} ${Display.address(e.local) || e.path || ""} ${Display.address(e.remote)} ${e.target} ${[...e.peers, ...e.holders].map((p) => `${p.process_id.pid} ${p.name}`).join(" ")}`
      .toLowerCase()
      .includes(search),
  );
}
export function bindProcessSearch(changed: (kind: DetailKind) => void) {
  const environment = () => changed("environment"),
    fds = () => changed("fds");
  $("environment-search").addEventListener("input", environment);
  $("fds-search").addEventListener("input", fds);
  return () => {
    $("environment-search").removeEventListener("input", environment);
    $("fds-search").removeEventListener("input", fds);
  };
}
