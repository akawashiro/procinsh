import { test } from "vitest";
import assert from "node:assert/strict";
import { Display } from "../src/shared/display.js";
import { remoteLabel, processColors } from "../src/space/model.js";
import { socketEndpoint } from "./support/fixtures.js";

test("space formatting regression", () => {
  assert.equal(
    remoteLabel(
      socketEndpoint({
        remote: { ip: "2001:db8::1", port: 443 },
        remote_hostname: "example.test",
      }),
    ),
    "example.test:443",
  );
  assert.equal(
    remoteLabel(socketEndpoint({ remote: { ip: "192.0.2.1", port: 80 } })),
    "192.0.2.1:80",
  );

  assert.equal(
    processColors({ uid: 1000, euid: 1000 }).real,
    processColors({ uid: 1000, euid: 1000 }).effective,
  );
  assert.notEqual(
    processColors({ uid: 1000, euid: 0 }).real,
    processColors({ uid: 1000, euid: 0 }).effective,
  );
  assert.equal(processColors({ uid: null, euid: null }).real, "#889299");
  assert.equal(Display.state({ kind: "unknown_inet", code: 255 }), "FF");
  assert.equal(
    Display.protocol({ kind: "unix", socket_type: { kind: "seqpacket" } }),
    "UNIX SEQPACKET",
  );
  assert.equal(Display.access("unknown"), "N/A");

  assert.equal(
    Display.affinity([
      { start: 0, end: 3 },
      { start: 8, end: 8 },
    ]),
    "0-3,8",
  );
  assert.equal(Display.affinity(null), "N/A");
  assert.equal(
    Display.scheduler({ kind: "unknown", code: 99 }),
    "UNKNOWN (99)",
  );

  assert.equal(
    Display.mapping({
      pathname: "/tmp/a [b]",
      readable: true,
      writable: true,
      executable: false,
      private: true,
    }),
    "/tmp/a [b] [rw-p]",
  );
  assert.equal(
    Display.mapping({
      pathname: null,
      readable: true,
      writable: false,
      executable: false,
      private: false,
    }),
    "[anonymous] [r--s]",
  );
});
