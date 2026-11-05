import { test } from "vitest";

import assert from "node:assert/strict";
import { remoteLabel, processColors } from "../src/space/model.js";
import { socketEndpoint } from "./support/fixtures.js";

test("remote labels and real/effective user colors", () => {
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
});
