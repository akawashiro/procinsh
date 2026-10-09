// Render selection details and identity-preserving process links in the DOM.
import { Display } from "../shared/display.js";
import { processUrl } from "../shared/navigation.js";
import {
  connectionState,
  fileLabel,
  ipcLabel,
  key,
  processColors,
  remoteLabel,
} from "./model.js";
import type { EdgeStat, SelectionState } from "./types.js";
import type { DetailsInput, SelectionActions } from "./contracts.js";
import type {
  FdEndpoint,
  Process,
  ProcessId,
  SocketEndpoint,
} from "../shared/api-types.js";
import { spaceElement } from "./dom-types.js";

function renderProcessIdentity(
  container: HTMLElement,
  identity: ProcessId,
  process?: Pick<Process, "uid" | "username" | "euid" | "effective_username">,
) {
  const colors = processColors(process ?? {});
  container.replaceChildren(document.createTextNode(`PID ${identity.pid} · `));
  for (const [role, uid, name, color] of [
    ["Real", process?.uid, process?.username, colors.real],
    ["Effective", process?.euid, process?.effective_username, colors.effective],
  ] as const) {
    const label = document.createElement("span");
    label.style.color = color;
    label.textContent = `${role}: ${name ?? uid ?? "unknown"}${name ? ` (${uid})` : ""} `;
    container.append(label);
  }
}

export function createSpaceDetails(
  actions: Pick<SelectionActions, "connection" | "network">,
  $ = spaceElement,
) {
  let nodes: DetailsInput["nodes"],
    snapshot: DetailsInput["snapshot"],
    network: DetailsInput["network"],
    edgeStats: DetailsInput["edgeStats"],
    recentFiles: DetailsInput["files"];
  let selected: string | null = null,
    selectedEdge: string | null = null,
    selectedNetwork: string | null = null,
    selectedFile: string | null = null;
  const selectConnection = actions.connection,
    selectNetwork = actions.network;
  function networkDetails() {
    const group = network.get(selectedNetwork ?? "");
    if (!group) return;
    $("process-details").hidden = true;
    $("connection-details").hidden = false;
    $("connection-label").textContent = group.label;
    $("connection-state").textContent = "Network destination";
    const stats = group.members
      .map((e) => edgeStats.get(e.id))
      .filter((s): s is EdgeStat => s !== undefined);
    const latest = Math.max(0, ...stats.map((s) => s.time)),
      recent = stats.filter((s) => s.time === latest);
    $("connection-facts").textContent = recent.length
      ? `Latest window: ${recent.reduce((n, s) => n + s.bytes, 0)} bytes / ${recent.reduce((n, s) => n + s.count, 0)} operations · ${((performance.now() - latest) / 1000).toFixed(1)}s ago`
      : "Recent traffic —";
    const content = document.createDocumentFragment(),
      heading = document.createElement("p");
    const remoteAddress =
      group.socket.remote_hostname && group.socket.remote
        ? ` (${Display.address(group.socket.remote)})`
        : "";
    heading.textContent = `${nodes.get(key(group.endpoint.process_id))?.name || ""} · PID ${group.endpoint.process_id.pid} → ${remoteLabel(group.socket)}${remoteAddress} · ${group.members.length} connections`;
    content.append(heading);
    for (const e of group.members) {
      const row = document.createElement("div");
      row.className = "endpoint";
      const button = document.createElement("button");
      button.textContent = `FD ${e.endpoint.fd}${e.endpoint.fd_count > 1 ? ` (+${e.endpoint.fd_count - 1} shared FDs)` : ""} · ${Display.state(e.socket!.state)}`;
      button.onclick = () => selectConnection(e.id);
      const address = document.createElement("span");
      address.textContent = `${Display.address(e.socket!.local) || e.socket!.path || "—"} → ${Display.address(e.socket!.remote)}`;
      const stat = edgeStats.get(e.id),
        observed = document.createElement("span");
      observed.textContent = stat
        ? `${stat.bytes} bytes / ${stat.count} operations · ${((performance.now() - stat.time) / 1000).toFixed(1)}s ago`
        : "Recent traffic —";
      row.append(button, address, observed);
      content.append(row);
    }
    $("connection-endpoints").replaceChildren(content);
  }
  function fileDetails() {
    const f = recentFiles.get(selectedFile ?? "");
    if (!f) return;
    $("process-details").hidden = true;
    $("connection-details").hidden = false;
    $("connection-label").textContent = f.label;
    $("connection-state").textContent = "Regular file";
    $("connection-facts").textContent =
      `Observed READ: ${f.readBytes} bytes / ${f.readCount} operations · WRITE: ${f.writeBytes} bytes / ${f.writeCount} operations`;
    const path = document.createElement("p");
    path.textContent = f.path || `Path unavailable · ${fileLabel(f.file)}`;
    const identity = document.createElement("p");
    identity.textContent = `${nodes.get(key(f.process_id))?.name || "Unknown process"} · PID ${f.process_id.pid} · ${fileLabel(f.file)}`;
    const link = document.createElement("a");
    link.href = processUrl(f.process_id);
    link.textContent = "Open process details ↗";
    updateConnectionEndpoints([path, identity, link]);
  }
  function endpoint(
    fdEndpoint: FdEndpoint | null,
    title: string,
    socket: SocketEndpoint | null = null,
  ) {
    const div = document.createElement("div");
    div.className = "endpoint";
    const n = fdEndpoint && nodes.get(key(fdEndpoint.process_id));
    const heading = document.createElement("strong");
    heading.textContent = fdEndpoint
      ? n?.name || `Unknown process`
      : socket?.network_peer
        ? (remoteLabel(socket) ?? "Unknown destination")
        : "External / unknown";
    div.append(heading);
    if (!fdEndpoint) {
      const note = document.createElement("span");
      note.textContent = socket?.network_peer
        ? `${Display.protocol(socket.protocol)} · ${Display.state(socket.state)}`
        : "The peer process could not be identified.";
      div.append(note);
      return div;
    }
    const identity = document.createElement("span");
    identity.textContent = `${title} · PID ${fdEndpoint.process_id.pid} · ${n?.username ?? n?.uid ?? "unknown"}`;
    const fd = document.createElement("span");
    fd.textContent = `FD ${fdEndpoint.fd}${fdEndpoint.fd_count > 1 ? ` (+${fdEndpoint.fd_count - 1} shared FDs)` : ""} · ${Display.access(fdEndpoint.access)}`;
    const resource = document.createElement("span");
    resource.textContent = ipcLabel(fdEndpoint.resource);
    const link = document.createElement("a");
    link.href = processUrl(fdEndpoint.process_id);
    link.textContent = "Open process details ↗";
    div.append(identity, fd, resource, link);
    return div;
  }
  // Keep unchanged links attached across live updates so pointer presses and
  // keyboard focus survive until the user activates them.
  function updateConnectionEndpoints(children: HTMLElement[]) {
    const container = $("connection-endpoints");
    children.forEach((child, index) => {
      const current = container.children[index];
      if (!current) container.append(child);
      else if (!current.isEqualNode(child)) current.replaceWith(child);
    });
    while (container.children.length > children.length) {
      container.lastElementChild!.remove();
    }
  }
  function details(data: DetailsInput, selection: SelectionState) {
    ({ nodes, snapshot, network, edgeStats, files: recentFiles } = data);
    selected = selection.process;
    selectedEdge = selection.connection;
    selectedNetwork = selection.network;
    selectedFile = selection.file;
    const active = !!(
      selected ||
      selectedEdge ||
      selectedNetwork ||
      selectedFile
    );
    $("details").hidden = !active;
    if (!active) return;
    $("connection-kind").textContent = selectedFile
      ? "SELECTED FILE"
      : "SELECTED CONNECTION";
    if (selectedFile) {
      fileDetails();
      return;
    }
    if (selectedNetwork) {
      networkDetails();
      return;
    }
    const process = $("process-details"),
      connection = $("connection-details");
    if (selected) {
      const n = nodes.get(selected ?? "");
      if (!n) return;
      process.hidden = false;
      connection.hidden = true;
      $("name").textContent = n.name;
      renderProcessIdentity($("pid"), n.identity, n);
      $("inspect").href = processUrl(n.identity);
      $("parent-details").hidden = !n.parent_id;
      if (n.parent_id) {
        renderProcessIdentity(
          $("parent-pid"),
          n.parent_id,
          nodes.get(key(n.parent_id)),
        );
        $("parent-inspect").href = processUrl(n.parent_id);
      } else {
        $("parent-pid").replaceChildren();
        $("parent-inspect").removeAttribute("href");
      }
      return;
    }
    const e = snapshot.fd_relations.find((edge) => edge.id === selectedEdge);
    if (!e) return;
    process.hidden = true;
    connection.hidden = false;
    $("connection-label").textContent = e.label;
    $("connection-state").textContent = connectionState(e);
    const stat = edgeStats.get(e.id);
    $("connection-facts").textContent = stat
      ? `Latest activity: ${stat.bytes} bytes / ${stat.count} operations · ${((performance.now() - stat.time) / 1000).toFixed(1)}s ago`
      : "Recent traffic —";
    const endpoints: HTMLElement[] = [
      endpoint(e.endpoint, "ENDPOINT"),
      endpoint(e.peer, "PEER", e.socket),
    ];
    if (e.socket) {
      const info = document.createElement("p");
      info.textContent = `${Display.protocol(e.socket.protocol)} ${Display.state(e.socket!.state)} · ${Display.address(e.socket!.local) || e.socket!.path || "—"} → ${Display.address(e.socket.remote) || "—"}`;
      endpoints.push(info);
    }
    const group = [...network.values()].find((g) =>
      g.members.some((member) => member.id === e.id),
    );
    if (group) {
      const back = document.createElement("button");
      back.textContent = "Show all connections to this destination";
      back.dataset.networkId = group.id;
      back.onclick = () => selectNetwork(group.id);
      endpoints.push(back);
    }
    updateConnectionEndpoints(endpoints);
  }
  return { update: details };
}
