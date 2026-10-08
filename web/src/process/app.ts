// Compose process data, rendering, and user actions; own page lifecycle events.
import { processUrl } from "../shared/navigation.js";
import { ProcessDataStore } from "./data.js";
import { createProcessRenderer } from "./renderer.js";
import { ThreadSelection, createThreadSelectionView } from "./selection.js";
import { createProcessSamples } from "./samples.js";
import { drawHistory } from "./history.js";
import { createProcessDetails } from "./details.js";
import { bindProcessSearch } from "./search.js";
import { processElement as $ } from "./dom-types.js";
const selection = new ThreadSelection();
const data = new ProcessDataStore({
  identity(id) {
    history.replaceState(null, "", processUrl(id));
  },
  reset() {
    selection.reset();
    renderer.reset();
    samples.reset();
    details.reset();
  },
  snapshot: render,
  detail(kind) {
    details.update(kind);
  },
  loading(show) {
    renderer.loading(show);
  },
  error(error) {
    renderer.error(error);
  },
});
const renderer = createProcessRenderer(data);
const threads = createThreadSelectionView(data, selection, render);
const samples = createProcessSamples(data, selection);
const details = createProcessDetails(data);
bindProcessSearch(details.render);
function renderHistory() {
  drawHistory($("history"), $("history-scale"), data.target?.history ?? []);
}
function render() {
  selection.retain(data.target);
  renderer.update();
  threads.update();
  samples.update();
  if (data.target?.observation) renderHistory();
}
addEventListener("pagehide", () => data.stop());
addEventListener("pageshow", (event) => {
  if (event.persisted) data.resume(location.pathname, location.search);
});
addEventListener("resize", renderHistory);
void data.start(location.pathname, location.search);
