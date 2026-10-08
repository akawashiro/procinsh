// Compose list data, search, and rendering, and own page lifecycle events.
import { ListDataStore } from "./data.js";
import { createListSearch } from "./search.js";
import { renderProcesses, renderError } from "./renderer.js";
import { listElement as $ } from "./dom-types.js";
const data = new ListDataStore({ changed: render, error: renderError });
const search = createListSearch(
  $("search"),
  $("sort"),
  () => data.processes,
  render,
);
function render() {
  renderProcesses(search.rows(), data.processes.length);
}
addEventListener("pagehide", () => data.stop());
addEventListener("pageshow", (event) => {
  if (event.persisted) void data.start();
});
void data.start();
