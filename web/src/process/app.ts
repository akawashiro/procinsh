// Compose process data, rendering, and user actions; own page lifecycle events.
import { processUrl } from "../shared/navigation.js";
import { watchMasked } from "../shared/privacy.js";
import { hostUrl, mountHostControls, withHost } from "../shared/hosts.js";
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
    history.replaceState(null, "", processUrl(id, data.host));
    links();
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
/** View links keep the host of this process. */
function links() {
  $("back").href = withHost("/list", data.host);
  $("space-link").href = withHost("/space", data.host);
}
// Another host has other processes; switching hosts goes to its list.
mountHostControls(
  $("host-picker"),
  () => data.host,
  (host) =>
    location.assign(hostUrl(host, new URL("/list", location.href).href)),
);
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
links();
watchMasked();
