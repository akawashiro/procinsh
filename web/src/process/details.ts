// Render and bind the environment, auxiliary-vector, and FD panels.
import { Display } from "../shared/display.js";
import { node, cell } from "../shared/dom.js";
import { processUrl } from "../shared/navigation.js";
import type {
  FileDescriptors,
  DescriptorEndpoint,
} from "../shared/api-types.js";
import { detailKinds } from "./data.js";
import type { ProcessDataStore, DetailKind } from "./data.js";
import { matchingEnvironment, matchingDescriptors } from "./search.js";
import { processElement as $ } from "./dom-types.js";
export function createProcessDetails(store: ProcessDataStore) {
  function renderProcessDetails(kind: DetailKind) {
    const data = store.details[kind].data;
    if (!data) {
      $(`${kind}-info`).textContent = "Not captured";
      return;
    }
    const time = new Date(data.captured_at).toLocaleTimeString("en-GB", {
      hour12: false,
    });
    if ("warnings" in data) {
      renderDescriptors(data, time);
      return;
    }
    if ("lossy_utf8" in data) {
      const entries = matchingEnvironment(
        data.entries,
        $("environment-search").value,
      );
      $("environment-info").textContent =
        `${entries.length} / ${data.entries.length} entries · ${time} · auto 5s${data.lossy_utf8 ? " · Invalid UTF-8 is shown as �" : ""}`;
      $("environment-entries").replaceChildren(
        ...entries.map((e) => {
          const row = node("tr");
          cell(row, e.name, "mono");
          cell(row, e.value ?? "(no = sign)", "mono");
          return row;
        }),
      );
      if (!entries.length) {
        const row = node("tr");
        cell(
          row,
          data.entries.length
            ? "No matching environment variables."
            : "The environment is empty.",
          "muted",
        ).colSpan = 2;
        $("environment-entries").append(row);
      }
    } else {
      $("auxv-info").textContent =
        `${data.entries.length} entries · ELF${data.word_bits} · ${time} · auto 5s`;
      $("auxv-entries").replaceChildren(
        ...data.entries.map((e) => {
          const row = node("tr");
          cell(row, `${e.name} (${e.tag})`, "mono");
          cell(row, e.value, "mono");
          cell(row, e.decimal, "mono muted");
          const description = cell(row, e.description);
          if (e.text != null)
            description.append(node("div", e.text, "mono auxv-string"));
          if (e.text_error)
            description.append(
              node("div", `String: N/A · ${e.text_error}`, "muted"),
            );
          return row;
        }),
      );
    }
  }
  function renderDescriptors(data: FileDescriptors, time: string) {
    const entries = matchingDescriptors(data.entries, $("fds-search").value);
    $("fds-info").textContent =
      `${entries.length} / ${data.entries.length} FDs · ${time} · auto 5s`;
    $("fds-warnings").textContent = data.warnings.join(" ");
    $("fds-warnings").hidden = !data.warnings.length;
    $("fds-entries").replaceChildren(
      ...entries.map((e) => {
        const row = node("tr");
        row.dataset.fd = String(e.fd);
        cell(row, e.fd, "mono");
        cell(
          row,
          `${Display.protocol(e.protocol) || e.kind}\n${Display.access(e.access)}`,
          "mono",
        );
        const resource = cell(row, e.target, "mono muted");
        if (e.state) resource.append(node("div", Display.state(e.state)));
        if (e.path) resource.append(node("div", `Path: ${e.path}`));
        if (e.local)
          resource.append(node("div", `Local: ${Display.address(e.local)}`));
        if (e.remote)
          resource.append(node("div", `Remote: ${Display.address(e.remote)}`));
        if (e.peer_inode)
          resource.append(node("div", `Peer inode: ${e.peer_inode}`));
        const peers = cell(row);
        const endpoint = (p: DescriptorEndpoint, group: string) => {
          const div = node("div", null, "fd-endpoint");
          div.dataset.relation = group;
          const link = node(
            "a",
            `PID ${p.process_id.pid} · ${p.name}`,
            "pointer",
          );
          link.href = processUrl(p.process_id);
          link.dataset.pid = String(p.process_id.pid);
          div.append(
            link,
            node(
              "div",
              `FD ${p.fd} · ${Display.access(p.access)} · ${p.relation}`,
              "muted",
            ),
          );
          return div;
        };
        for (const p of e.peers) peers.append(endpoint(p, "peer"));
        if (e.note) peers.append(node("div", e.note, "muted"));
        if (e.holders.length) {
          const shared = node("details", null, "fd-holders");
          shared.append(
            node(
              "summary",
              `Holders of the same FD resource (${e.holders.length})`,
            ),
          );
          for (const p of e.holders) shared.append(endpoint(p, "holder"));
          peers.append(shared);
        }
        return row;
      }),
    );
    if (!entries.length) {
      const row = node("tr");
      cell(
        row,
        data.entries.length
          ? "No matching FDs."
          : "No observable pipes or sockets.",
        "muted",
      ).colSpan = 4;
      $("fds-entries").append(row);
    }
  }

  function reset() {
    for (const kind of detailKinds) {
      $(`${kind}-panel`).open = false;
      $(`${kind}-entries`).replaceChildren();
      $(`${kind}-error`).hidden = true;
      $(`${kind}-info`).textContent = "Not captured";
    }
    $("environment-search").value = "";
    $("fds-search").value = "";
    $("fds-warnings").hidden = true;
  }
  const handlers = detailKinds.map((kind) => {
    const toggle = () => store.setPanelOpen(kind, $(`${kind}-panel`).open);
    $(`${kind}-panel`).addEventListener("toggle", toggle);
    return () => $(`${kind}-panel`).removeEventListener("toggle", toggle);
  });
  return {
    reset,
    render: renderProcessDetails,
    update(kind: DetailKind) {
      const view = store.details[kind];
      $(`${kind}-error`).hidden = view.error === null;
      if (view.error !== null) $(`${kind}-error`).textContent = view.error;
      if (view.busy) $(`${kind}-info`).textContent = "Reading…";
      else renderProcessDetails(kind);
    },
    dispose() {
      handlers.forEach((remove) => remove());
    },
  };
}
