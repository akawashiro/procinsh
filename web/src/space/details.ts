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
  RdmaEndpoint,
  RdmaRelation,
  SocketEndpoint,
} from "../shared/api-types.js";
import { rdmaDeviceId, rdmaEndpointLabel, rdmaOwnerLabel } from "./rdma.js";
import { spaceElement } from "./dom-types.js";
import { deviceUsers, downgraded, pathLabel } from "./hardware.js";
export function createSpaceDetails(
  actions: Pick<SelectionActions, "connection" | "network">,
  $ = spaceElement,
) {
  let nodes: DetailsInput["nodes"],
    snapshot: DetailsInput["snapshot"],
    network: DetailsInput["network"],
    edgeStats: DetailsInput["edgeStats"],
    recentFiles: DetailsInput["files"],
    netlinkSockets: DetailsInput["netlink"],
    hardware: DetailsInput["hardware"],
    rdma: DetailsInput["rdma"],
    host: string | null = null;
  let selected: string | null = null,
    selectedEdge: string | null = null,
    selectedNetwork: string | null = null,
    selectedFile: string | null = null,
    selectedNetlink: string | null = null,
    selectedHardware: string | null = null,
    selectedRdma: string | null = null;
  const selectConnection = actions.connection,
    selectNetwork = actions.network;
  const replayMode = document.body.dataset.mode === "replay";
  function processLink(id: ProcessId) {
    const link = document.createElement("a");
    link.className = "process-link";
    if (!replayMode) link.href = processUrl(id, host);
    link.textContent = nodes.get(key(id))?.name || "Unknown process";
    return link;
  }
  /** `host · ` for a process of the merged multi-host view. */
  const hostPrefix = (id: ProcessId) =>
    id.host === undefined ? "" : `${id.host} · `;
  /** Local port addresses, so both ends of an IB path are visible together. */
  function localPort(endpoint: RdmaEndpoint) {
    const port = rdma?.devices
      .get(rdmaDeviceId(endpoint.device, endpoint.host))
      ?.ports.find((p) => p.port === endpoint.port);
    if (!port) return null;
    return [
      port.lid === null ? null : `LID ${port.lid}`,
      port.gid && `GID ${port.gid}`,
    ]
      .filter(Boolean)
      .join(" · ");
  }
  function rdmaOwner(endpoint: RdmaEndpoint) {
    const heading = document.createElement("strong");
    if (endpoint.owner.kind === "process")
      heading.append(processLink(endpoint.owner.process_id));
    else
      heading.textContent =
        endpoint.owner.kind === "kernel"
          ? `Kernel · ${endpoint.owner.name}`
          : rdmaOwnerLabel(endpoint.owner);
    return heading;
  }
  function rdmaEndpointCard(endpoint: RdmaEndpoint, title: string) {
    const div = document.createElement("div");
    div.className = "endpoint";
    div.append(
      rdmaOwner(endpoint),
      line(
        `${title} · ${rdmaEndpointLabel(endpoint)} · ${Display.rdmaQpType(endpoint.qp_type)} · ${Display.rdmaState(endpoint.state)}`,
      ),
    );
    const local = localPort(endpoint);
    if (local) div.append(line(`Local ${local}`));
    return div;
  }
  function rdmaQueuePair(relation: RdmaRelation) {
    const div = rdmaEndpointCard(relation.endpoint, "QP");
    const remote = [
      relation.remote_qpn === null ? null : `Remote QP ${relation.remote_qpn}`,
      relation.remote_gid && `GID ${relation.remote_gid}`,
    ].filter(Boolean);
    div.append(
      line(remote.length ? remote.join(" · ") : "Remote QP unavailable"),
    );
    const cm = relation.cm;
    if (cm) {
      const owner =
        cm.owner.kind === relation.endpoint.owner.kind &&
        rdmaOwnerLabel(cm.owner) === rdmaOwnerLabel(relation.endpoint.owner)
          ? ""
          : ` · ${rdmaOwnerLabel(cm.owner)}`;
      div.append(
        line(
          `CM ${Display.rdmaState(cm.state)} · ${Display.rdmaPortSpace(cm.port_space)}${owner} · ${Display.rdmaAddress(cm.source) || "—"} → ${Display.rdmaAddress(cm.destination) || "—"}${relation.remote_hostname ? ` (${relation.remote_hostname})` : ""}`,
        ),
      );
    }
    return div;
  }
  function rdmaDetails() {
    const id = selectedRdma ?? "";
    const group = rdma?.groups.get(id),
      link = rdma?.links.find((l) => l.id === id),
      device = rdma?.devices.get(id);
    if (!group && !link && !device) return;
    $("process-details").hidden = true;
    $("connection-details").hidden = false;
    $("connection-facts").textContent =
      "RDMA data transfers bypass the kernel; per-QP traffic is not observed.";
    if (device) {
      $("connection-label").textContent = device.label;
      $("connection-state").textContent = "RDMA device";
      updateConnectionEndpoints(
        device.ports.map((port) => {
          const div = document.createElement("div");
          div.className = "endpoint";
          const heading = document.createElement("strong");
          heading.textContent = `Port ${port.port} · ${port.state ?? "state unavailable"}`;
          div.append(
            heading,
            line(
              [port.link_layer, port.rate].filter(Boolean).join(" · ") ||
                "Link unavailable",
            ),
            line(
              [
                port.lid === null ? null : `LID ${port.lid}`,
                port.gid && `GID ${port.gid}`,
              ]
                .filter(Boolean)
                .join(" · ") || "No address assigned",
            ),
          );
          return div;
        }),
      );
      return;
    }
    if (link) {
      $("connection-label").textContent = link.relation.label;
      $("connection-state").textContent =
        link.relation.endpoint.host !== link.relation.peer?.host
          ? "Connected queue pairs on two hosts"
          : "Connected queue pairs on this host";
      updateConnectionEndpoints([
        rdmaEndpointCard(link.relation.endpoint, "ENDPOINT"),
        ...(link.relation.peer
          ? [rdmaEndpointCard(link.relation.peer, "PEER")]
          : []),
      ]);
      return;
    }
    $("connection-label").textContent = group!.label;
    $("connection-state").textContent = group!.remote
      ? "RDMA queue pairs to a remote destination"
      : "RDMA queue pairs";
    updateConnectionEndpoints(group!.members.map(rdmaQueuePair));
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
    if (group.socket.remote_gid) {
      const gid = document.createElement("p");
      gid.textContent = `InfiniBand GID ${group.socket.remote_gid} (IPoIB neighbor)`;
      content.push(gid);
    }
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
  function line(text: string) {
    const span = document.createElement("span");
    span.textContent = text;
    return span;
  }
  function card(title: string | HTMLElement, lines: string[]) {
    const div = document.createElement("div");
    div.className = "endpoint";
    const heading = document.createElement("strong");
    heading.append(title);
    div.append(heading, ...lines.filter(Boolean).map(line));
    return div;
  }
  const bytes = (n: number) =>
    n >= 1 << 30
      ? `${n / (1 << 30)} GiB`
      : n >= 1 << 20
        ? `${n / (1 << 20)} MiB`
        : `${n / 1024} KiB`;
  function hardwareDetails() {
    const devices = hardware?.devices ?? [];
    const device = devices.find((d) => d.address === selectedHardware);
    if (!device) return;
    const label = (address: string) =>
      devices.find((d) => d.address === address)?.label ?? address;
    $("process-details").hidden = true;
    $("connection-details").hidden = false;
    $("connection-label").textContent = device.label;
    const role = {
      host_bridge: "PCIe host bridge",
      root_port: "PCIe root port",
      switch_port: "PCIe switch port",
      endpoint: {
        gpu: "GPU",
        rdma: "RDMA NIC",
        accelerator: "Accelerator",
        nv_switch: "NVSwitch",
      }[device.kind ?? "gpu"],
    }[device.role];
    $("connection-state").textContent = `${role} · ${device.address}`;
    const parent = devices.find((d) => d.address === device.parent);
    $("connection-facts").textContent = device.link
      ? `PCIe ${Display.pcieLink(device.link.current)} · max ${Display.pcieLink(device.link.max)}${downgraded(device, parent) ? " · running below the maximum of both ends" : ""}`
      : device.role === "host_bridge"
        ? "Root complex"
        : `Link unavailable${device.runtime_status === "suspended" ? " · runtime suspended" : ""}`;
    const cards: HTMLElement[] = [
      card("Device", [
        [device.vendor_id, device.device_id].filter(Boolean).join(":") &&
          `ID ${device.vendor_id}:${device.device_id} · class ${device.class ?? "N/A"}`,
        device.driver ? `Driver ${device.driver}` : "",
        `NUMA ${device.numa_node ?? "N/A"}${device.iommu_group === null ? "" : ` · IOMMU group ${device.iommu_group}`}`,
        device.power_state || device.runtime_status
          ? `Power ${[device.power_state, device.runtime_status].filter(Boolean).join(" · ")}`
          : "",
        device.bars.length
          ? `BAR ${device.bars.map((b) => `${b.index}: ${bytes(b.bytes)}${b.prefetchable ? " (prefetchable)" : ""}`).join(", ")}`
          : "",
        device.gpu
          ? `GPU ${device.gpu.vendor}${device.gpu.uuid ? ` · ${device.gpu.uuid}` : ""}${device.gpu.drm.length ? ` · ${device.gpu.drm.join(", ")}` : ""}`
          : "",
        ...device.rdma.map(
          (r) =>
            `RDMA ${r.name}${r.hca_type ? ` · ${r.hca_type}` : ""}${r.fw_ver ? ` · firmware ${r.fw_ver}` : ""}`,
        ),
        device.netdevs.length ? `Network ${device.netdevs.join(", ")}` : "",
      ]),
    ];
    const below = devices.filter((d) => d.parent === device.address);
    if (below.length)
      cards.push(
        card(
          "Downstream",
          below.map((d) => `${d.label} · ${d.address}`),
        ),
      );
    for (const path of hardware?.paths ?? []) {
      if (path.a !== device.address && path.b !== device.address) continue;
      const other = path.a === device.address ? path.b : path.a;
      const via =
        path.a === device.address ? path.via : [...path.via].reverse();
      cards.push(
        card(`Path to ${label(other)}`, [
          pathLabel(path),
          `via ${via.join(" → ")}`,
        ]),
      );
    }
    const users = deviceUsers(
      (hardware?.handles ?? []).filter((h) => h.device === device.address),
      (h) => key(h.process_id),
    ).get(device.address);
    for (const handles of users?.values() ?? []) {
      const heading = document.createElement("span");
      heading.append(processLink(handles[0].process_id));
      cards.push(
        card(heading, [
          `PID ${handles[0].process_id.pid} · ${handles.map((h) => `${h.kind} ×${h.fd_count} (FD ${h.fd})`).join(", ")}`,
        ]),
      );
    }
    updateConnectionEndpoints(cards);
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
      if (socket?.remote_gid) {
        const gid = document.createElement("span");
        gid.textContent = `InfiniBand GID ${socket.remote_gid}`;
        div.append(gid);
      }
      return div;
    }
    const identity = document.createElement("span");
    identity.textContent = `${title} · ${hostPrefix(fdEndpoint.process_id)}PID ${fdEndpoint.process_id.pid} · ${n?.username ?? n?.uid ?? "unknown"}`;
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
      hardware,
      rdma,
    } = data);
    host = data.host ?? null;
    selected = selection.process;
    selectedEdge = selection.connection;
    selectedNetwork = selection.network;
    selectedFile = selection.file;
    selectedNetlink = selection.netlink;
    selectedHardware = selection.hardware ?? null;
    selectedRdma = selection.rdma ?? null;
    const active = !!(
      selected ||
      selectedEdge ||
      selectedNetwork ||
      selectedFile ||
      selectedNetlink ||
      selectedHardware ||
      selectedRdma
    );
    $("details").hidden = !active;
    if (!active) return;
    $("connection-state").hidden = !!selectedNetlink;
    if (selectedHardware) {
      hardwareDetails();
      return;
    }
    if (selectedRdma) {
      rdmaDetails();
      return;
    }
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
      if (!replayMode) $("name").href = processUrl(n.identity, host);
      else $("name").removeAttribute("href");
      $("cmdline").textContent = n.command_line?.join(" ") || "N/A";
      $("pid").replaceChildren(
        document.createTextNode(
          `${hostPrefix(n.identity)}PID ${n.identity.pid} · `,
        ),
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
