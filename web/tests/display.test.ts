import { describe, expect, test } from "vitest";

import { bytes, byteRate, Display, percent, rate } from "../src/shared/display";
import type {
  RdmaCmState,
  RdmaPortSpace,
  RdmaQpState,
  RdmaQpType,
  SocketProtocol,
} from "../src/shared/api-types.js";
import { readFileSync } from "node:fs";

const socketProtocols = JSON.parse(
  readFileSync(
    new URL(
      "../../src/privileged_helper/test_support/socket-protocols.json",
      import.meta.url,
    ),
    "utf8",
  ),
) as { value: SocketProtocol; label: string }[];
const rdmaCodes = JSON.parse(
  readFileSync(
    new URL(
      "../../src/privileged_helper/test_support/rdma-codes.json",
      import.meta.url,
    ),
    "utf8",
  ),
) as {
  group: "qp_type" | "qp_state" | "cm_state" | "port_space";
  code: number;
  value: RdmaQpType & RdmaQpState & RdmaCmState & RdmaPortSpace;
  label: string;
}[];

describe("shared formatting", () => {
  test.each(rdmaCodes)(
    "formats RDMA $group $code as $label",
    ({ group, value, label }) => {
      const format = {
        qp_type: Display.rdmaQpType,
        qp_state: Display.rdmaState,
        cm_state: Display.rdmaState,
        port_space: Display.rdmaPortSpace,
      }[group];
      expect(format(value)).toBe(label);
    },
  );
  test("formats RDMA CM addresses like the server", () => {
    expect(
      Display.rdmaAddress({ kind: "inet", ip: "10.10.10.1", port: 20049 }),
    ).toBe("10.10.10.1:20049");
    expect(
      Display.rdmaAddress({
        kind: "ib",
        gid: "fe80:0000:0000:0000:0000:0000:0000:0051",
        service_id: "0x0000000001064e51",
        pkey: 65535,
      }),
    ).toBe("fe80:0000:0000:0000:0000:0000:0000:0051 SID 0x0000000001064e51");
    expect(Display.rdmaAddress({ kind: "unknown", family: 42 })).toBe("AF 42");
    expect(Display.rdmaAddress(null)).toBe("");
  });
  test.each(socketProtocols)(
    "formats $label with no peer information",
    ({ value, label }) => {
      expect(Display.protocol(value)).toBe(label);
    },
  );
  test("distinguishes missing observations from zero", () => {
    for (const format of [bytes, byteRate, percent, rate]) {
      expect(format(null)).toBe("N/A");
      expect(format(undefined)).toBe("N/A");
    }
    expect(bytes(0)).toBe("0 B");
    expect(percent(0)).toBe("0%");
    expect(rate(0)).toBe("0/s");
  });

  test.each([
    [1023, "1,023 B"],
    [1024, "1 KiB"],
    [1536, "1.5 KiB"],
    [1024 ** 4, "1 TiB"],
  ])("formats %i bytes as %s", (value, expected) => {
    expect(bytes(value)).toBe(expected);
    expect(byteRate(value)).toBe(`${expected}/s`);
  });

  test("formats descriptor access and memory mappings", () => {
    expect(Display.access("unknown")).toBe("N/A");
    expect(
      Display.mapping({
        pathname: "/tmp/a [b]",
        readable: true,
        writable: true,
        executable: false,
        private: true,
      }),
    ).toBe("/tmp/a [b] [rw-p]");
    expect(
      Display.mapping({
        pathname: null,
        readable: true,
        writable: false,
        executable: false,
        private: false,
      }),
    ).toBe("[anonymous] [r--s]");
  });

  test("formats IPv6 addresses and unknown structured values", () => {
    expect(Display.address({ ip: "::1", port: 9090 })).toBe("[::1]:9090");
    expect(Display.address({ ip: "127.0.0.1", port: 9090 })).toBe(
      "127.0.0.1:9090",
    );
    expect(Display.address(null)).toBe("");
    expect(Display.affinity(null)).toBe("N/A");
    expect(Display.scheduler({ kind: "unknown", code: 99 })).toBe(
      "UNKNOWN (99)",
    );
    expect(Display.state({ kind: "unknown_inet", code: 10 })).toBe("0A");
    expect(
      Display.affinity([
        { start: 0, end: 2 },
        { start: 4, end: 4 },
      ]),
    ).toBe("0-2,4");
  });
});
