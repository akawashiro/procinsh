// Compose list data, search, and rendering, and own page lifecycle events.
import { ListDataStore } from "./data.js";
import { watchMasked } from "../shared/privacy.js";
import { createListSearch } from "./search.js";
import { renderProcesses, renderError } from "./renderer.js";
import { listElement as $ } from "./dom-types.js";
import {
  hostUrl,
  mountHostControls,
  readHostParam,
  withHost,
} from "../shared/hosts.js";
const data = new ListDataStore({ changed: render, error: renderError });
const search = createListSearch(
  $("search"),
  $("sort"),
  () => data.processes,
  render,
);
function render() {
  renderProcesses(search.rows(), data.processes.length, data.host);
}
function selectHost(host: string | null) {
  data.setHost(host);
  $("space-link").href = withHost("/space", host);
  controls.refresh();
}
const controls = mountHostControls(
  $("host-picker"),
  () => data.host,
  (host) => {
    history.pushState(null, "", hostUrl(host));
    selectHost(host);
  },
);
addEventListener("popstate", () => selectHost(readHostParam()));
selectHost(readHostParam());
addEventListener("pagehide", () => data.stop());
addEventListener("pageshow", (event) => {
  if (event.persisted) void data.start();
});
void data.start();
watchMasked();
