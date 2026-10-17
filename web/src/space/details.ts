// Render selection details and identity-preserving process links in the DOM.
import { spaceNow } from "./clock.js";
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
  ProcessId,
  SocketEndpoint,
} from "../shared/api-types.js";
import { spaceElement } from "./dom-types.js";
export function createSpaceDetails(
  actions: Pick<SelectionActions, "connection" | "network">,
  $ = spaceElement,
) {
  let nodes: DetailsInput["nodes"],
    snapshot: DetailsInput["snapshot"],
    network: DetailsInput["network"],
    edgeStats: DetailsInput["edgeStats"],
    recentFiles: DetailsInput["files"],
    netlinkSockets: DetailsInput["netlink"];
  let selected: string | null = null,
    selectedEdge: string | null = null,
    selectedNetwork: string | null = null,
    selectedFile: string | null = null,
    selectedNetlink: string | null = null;
  const selectConnection = actions.connection,
    selectNetwork = actions.network;
  const replayMode = document.body.dataset.mode === "replay";
  function processLink(id: ProcessId) {
    const link = document.createElement("a");
    link.className = "process-link";
    if (!replayMode) link.href = processUrl(id);
    link.textContent = nodes.get(key(id))?.name || "Unknown process";
    return link;
  }
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
      ? `Latest window: ${recent.reduce((n, s) => n + s.bytes, 0)} bytes / ${recent.reduce((n, s) => n + s.count, 0)} operations · ${((spaceNow() - latest) / 1000).toFixed(1)}s ago`
      : "Recent traffic —";
    const heading = document.createElement("p");
    const remoteAddress =
      group.socket.remote_hostname && group.socket.remote
        ? ` (${Display.address(group.socket.remote)})`
        : "";
    heading.append(
      processLink(group.endpoint.process_id),
      ` · PID ${group.endpoint.process_id.pid} → ${remoteLabel(group.socket)}${remoteAddress} · ${group.members.length} connections`,
    );
    const content: HTMLElement[] = [heading];
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
        ? `${stat.bytes} bytes / ${stat.count} operations · ${((spaceNow() - stat.time) / 1000).toFixed(1)}s ago`
        : "Recent traffic —";
      row.append(button, address, observed);
      content.push(row);
    }
    updateConnectionEndpoints(content);
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
    identity.append(
      processLink(f.process_id),
      ` · PID ${f.process_id.pid} · ${fileLabel(f.file)}`,
    );
    updateConnectionEndpoints([path, identity]);
  }
  function netlinkDetails() {
    const socket = netlinkSockets.get(selectedNetlink ?? "");
    if (!socket) return;
    $("process-details").hidden = true;
    $("connection-details").hidden = false;
    $("connection-label").textContent = socket.label;
    $("connection-state").textContent = "";
    $("connection-facts").textContent =
      `Observed SEND: ${socket.writeBytes} bytes / ${socket.writeCount} operations · RECEIVE: ${socket.readBytes} bytes / ${socket.readCount} operations`;
    const children: HTMLElement[] = [endpoint(socket.endpoint, "OWNER")];
    for (const holder of socket.holders)
      if (
        holder.process_id.pid !== socket.endpoint.process_id.pid ||
        holder.process_id.start_time_ticks !==
          socket.endpoint.process_id.start_time_ticks
      )
        children.push(endpoint(holder, "SHARED HOLDER"));
    updateConnectionEndpoints(children);
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
    if (fdEndpoint) heading.append(processLink(fdEndpoint.process_id));
    else
      heading.textContent = socket?.network_peer
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
    div.append(identity, fd, resource);
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
    ({
      nodes,
      snapshot,
      network,
      edgeStats,
      files: recentFiles,
      netlink: netlinkSockets,
    } = data);
    selected = selection.process;
    selectedEdge = selection.connection;
    selectedNetwork = selection.network;
    selectedFile = selection.file;
    selectedNetlink = selection.netlink;
    const active = !!(
      selected ||
      selectedEdge ||
      selectedNetwork ||
      selectedFile ||
      selectedNetlink
    );
    $("details").hidden = !active;
    if (!active) return;
    $("connection-state").hidden = !!selectedNetlink;
    if (selectedNetlink) {
      netlinkDetails();
      return;
    }
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
      if (!replayMode) $("name").href = processUrl(n.identity);
      else $("name").removeAttribute("href");
      $("cmdline").textContent = n.command_line?.join(" ") || "N/A";
      $("pid").replaceChildren(
        document.createTextNode(`PID ${n.identity.pid} · `),
      );
      for (const [role, uid, name, color] of [
        ["Real", n.uid, n.username, processColors(n).real],
        ["Effective", n.euid, n.effective_username, processColors(n).effective],
      ] as const) {
        const label = document.createElement("span");
        label.style.color = color;
        label.textContent = `${role}: ${name ?? uid ?? "unknown"}${name ? ` (${uid})` : ""} `;
        $("pid").append(label);
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
      ? `Latest activity: ${stat.bytes} bytes / ${stat.count} operations · ${((spaceNow() - stat.time) / 1000).toFixed(1)}s ago`
      : "Recent traffic —";
    // Collection can reverse a relation; order the cards by identity and FD.
    const [first, second] = e.peer
      ? [e.endpoint, e.peer].sort(
          (a, b) =>
            a.process_id.pid - b.process_id.pid ||
            a.process_id.start_time_ticks - b.process_id.start_time_ticks ||
            a.fd - b.fd,
        )
      : [e.endpoint, null];
    const endpoints: HTMLElement[] = [
      endpoint(first, "ENDPOINT"),
      endpoint(second, "PEER", e.socket),
    ];
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
