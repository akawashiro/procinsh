// @vitest-environment jsdom
import { test, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { ProcessDataStore } from "../src/process/data.js";
import { createProcessDetails } from "../src/process/details.js";
import { createSpaceDetails } from "../src/space/details.js";
import { spaceElement } from "../src/space/dom-types.js";
import {
  fdRelation,
  fdEndpoint,
  processInfo,
  socketEndpoint,
} from "./support/fixtures.js";
import type { SocketProtocol } from "../src/shared/api-types.js";
import type { DetailsInput } from "../src/space/contracts.js";
import type { SelectionState } from "../src/space/types.js";

const cases = JSON.parse(
  readFileSync(
    resolve(
      import.meta.dirname,
      "../../src/privileged_helper/test_support/socket-protocols.json",
    ),
    "utf8",
  ),
) as { value: SocketProtocol; label: string }[];
const identity = { pid: 2107, start_time_ticks: 770 };

test.each([...cases, { value: null, label: "Socket · Type unavailable" }])(
  "shows $label in FD and connection details without a destination",
  ({ value, label }) => {
    document.body.innerHTML = readFileSync(
      resolve(import.meta.dirname, "../process/index.html"),
      "utf8",
    );
    const store = new ProcessDataStore();
    store.details.fds.data = {
      process_id: identity,
      captured_at: 0,
      warnings: [],
      entries: [
        {
          fd: 11,
          kind: "socket",
          inode: "14130",
          target: "socket:[14130]",
          access: "read_write",
          protocol: value,
          state: null,
          local: null,
          remote: null,
          path: null,
          peer_inode: null,
          peers: [],
          holders: [],
          note: "Peer PID unknown",
        },
      ],
    };
    const processDetails = createProcessDetails(store);
    processDetails.render("fds");
    expect(
      document.querySelector('#fds-entries tr[data-fd="11"]')!.textContent,
    ).toContain(label);
    expect(
      document.querySelectorAll('#fds-entries [data-relation="peer"]'),
    ).toHaveLength(0);
    processDetails.dispose();
    store.stop();

    document.body.innerHTML = readFileSync(
      resolve(import.meta.dirname, "../space/index.html"),
      "utf8",
    );
    const edge = fdRelation({
      id: "kind-only",
      label,
      peer: null,
      endpoint: fdEndpoint({ process_id: identity, fd: 11 }),
      socket: value
        ? socketEndpoint({
            protocol: value,
            state: null,
            local: null,
            remote: null,
            path: null,
            network_peer: false,
          })
        : null,
    });
    const input: DetailsInput = {
      nodes: new Map([
        [
          "2107:770",
          {
            ...processInfo({ identity, name: "snapd" }),
            pos: { x: 0, y: 0, z: 0 },
            regions: [],
          },
        ],
      ]),
      snapshot: { processes: [], fd_relations: [edge] },
      network: new Map(),
      edgeStats: new Map(),
      netlink: new Map(),
      files: new Map(),
    };
    const selection: SelectionState = {
      process: null,
      connection: edge.id,
      network: null,
      netlink: null,
      file: null,
    };
    createSpaceDetails({ connection() {}, network() {} }, spaceElement).update(
      input,
      selection,
    );
    expect(spaceElement("connection-label").textContent).toBe(label);
    expect(spaceElement("connection-state").textContent).toBe(
      "Unknown destination",
    );
    expect(spaceElement("connection-endpoints").textContent).toContain("snapd");
    expect(spaceElement("connection-endpoints").textContent).toContain(
      "could not be identified",
    );
    expect(spaceElement("connection-endpoints").querySelector("p")).toBeNull();
    expect(spaceElement("connection-endpoints").textContent).not.toContain(
      "ESTABLISHED",
    );
  },
);
