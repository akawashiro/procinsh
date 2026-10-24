import { describe, expect, test } from "vitest";
import { bytes, byteRate, Display, percent, rate } from "../src/shared/display";
import type { SocketProtocol } from "../src/shared/api-types.js";
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

describe("shared formatting", () => {
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

  test("formats IPv6 addresses and unknown structured values", () => {
    expect(Display.address({ ip: "::1", port: 9090 })).toBe("[::1]:9090");
    expect(Display.address({ ip: "127.0.0.1", port: 9090 })).toBe(
      "127.0.0.1:9090",
    );
    expect(Display.address(null)).toBe("");
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
