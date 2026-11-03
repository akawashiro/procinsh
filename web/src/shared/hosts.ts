// The `?host=` selection shared by the list, process, and SPACE pages.

import { api } from "./api.js";
import type { HostsResponse } from "./api-types.js";

/** The `?host=` value of the merged SPACE view. */
export const ALL_HOSTS = "all";

/** The `?host=` value of `url`; null selects this machine. */
export function readHostParam(url = location.href): string | null {
  return new URL(url).searchParams.get("host") || null;
}

/** `url` with `?host=` set for a remote host or the merged view, or removed. */
export function hostUrl(host: string | null, url = location.href) {
  const next = new URL(url);
  if (host === null) next.searchParams.delete("host");
  else next.searchParams.set("host", host);
  return next.href;
}

/** An API or page path with `host` added to its query; null leaves it unchanged. */
export function withHost(path: string, host: string | null | undefined) {
  if (!host) return path;
  return `${path}${path.includes("?") ? "&" : "?"}${new URLSearchParams({ host })}`;
}

export interface HostControlsOptions {
  /** Offer "All hosts (layers)" when more than one host is known. */
  all?: boolean;
  readHosts?: () => Promise<HostsResponse>;
  interval?: number;
}

/**
 * Adds a host `<select>` to `container`, refreshed from `/api/hosts`. It stays
 * hidden while this machine is the only host and no other host is selected.
 */
export function mountHostControls(
  container: Element,
  current: () => string | null,
  change: (host: string | null) => void,
  {
    all = false,
    readHosts = () => api<HostsResponse>("/api/hosts"),
    interval = 5000,
  }: HostControlsOptions = {},
) {
  const wrapper = document.createElement("div");
  wrapper.id = "host-controls";
  wrapper.hidden = true;
  const select = document.createElement("select");
  select.id = "host";
  select.setAttribute("aria-label", "Host");
  wrapper.append(select);
  container.prepend(wrapper);
  const option = (value: string, text: string) => {
    const element = document.createElement("option");
    element.value = value;
    element.textContent = text;
    return element;
  };
  async function refresh() {
    let hosts: HostsResponse;
    try {
      hosts = await readHosts();
    } catch {
      return;
    }
    const selected = current();
    const options = hosts.hosts.map((h) =>
      option(
        h.local ? "" : h.name,
        `${h.name}${h.local ? " (local)" : ""}${h.status.state === "offline" ? " (offline)" : ""}`,
      ),
    );
    if (all && hosts.hosts.length > 1)
      options.push(option(ALL_HOSTS, "All hosts (layers)"));
    if (
      selected !== null &&
      !options.some((element) => element.value === selected)
    )
      options.push(option(selected, `${selected} (unavailable)`));
    select.replaceChildren(...options);
    select.value = selected ?? "";
    wrapper.hidden = hosts.hosts.length <= 1 && selected === null;
  }
  select.onchange = () => change(select.value || null);
  const timer = setInterval(refresh, interval);
  void refresh();
  return {
    refresh,
    dispose() {
      clearInterval(timer);
      wrapper.remove();
    },
  };
}
