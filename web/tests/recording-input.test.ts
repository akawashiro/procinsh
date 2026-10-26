import { expect, test } from "vitest";
import { gzipSync } from "node:zlib";
import { readFileSync } from "node:fs";
import { readRecording } from "../src/space/recording-input.js";
import { parseRecording } from "../src/space/recording.js";

const json = readFileSync(
  new URL("./fixtures/space-recording.json", import.meta.url),
  "utf8",
);
test.each(["chromium", "firefox"])(
  "published real %s gzip passes the shared schema and retains activity",
  async (browser) => {
    const gzip = readFileSync(
      new URL(
        `../replay-public/recordings/${browser}-ubuntu26.json.gz`,
        import.meta.url,
      ),
    );
    const recording = parseRecording(await readRecording(chunks(gzip, 65536)));
    expect(
      recording.initial_snapshot.processes.filter(
        (p) =>
          p.name.toLowerCase().includes(browser) ||
          p.command_line?.[0]?.toLowerCase().includes(browser),
      ).length,
    ).toBeGreaterThan(4);
    const activity = recording.events.filter(
      (event) => event.type === "activity",
    );
    for (const sensor of ["cpu", "ipc", "files"] as const)
      expect(
        activity.some((event) => (event.data[sensor]?.length ?? 0) > 0),
      ).toBe(true);
  },
);
test("published kernel build contains 30 seconds of real compiler activity", async () => {
  const gzip = readFileSync(
    new URL(
      "../replay-public/recordings/linux-build-ubuntu26.json.gz",
      import.meta.url,
    ),
  );
  expect(gzip.length).toBeLessThanOrEqual(25 * 1024 * 1024);
  const recording = parseRecording(await readRecording(chunks(gzip, 65536)));
  expect(recording.end_reason).toBe("Stopped by user");
  expect(recording.duration_ms).toBeGreaterThanOrEqual(30000);
  const snapshots = [
    recording.initial_snapshot,
    ...recording.events
      .filter((event) => event.type === "snapshot")
      .map((event) => event.data),
  ];
  expect(
    snapshots.every((snapshot) =>
      snapshot.processes.every(
        (process) =>
          !process.name.includes("ccache") &&
          !(process.command_line ?? []).some((argument) =>
            argument.includes("ccache"),
          ),
      ),
    ),
  ).toBe(true);
  expect(
    snapshots.some((snapshot) =>
      snapshot.processes.some((process) =>
        process.command_line?.includes("CC=clang"),
      ),
    ),
  ).toBe(true);
  const compilers = new Set(
    snapshots
      .flatMap((snapshot) => snapshot.processes)
      .filter((process) =>
        /^clang(?:-\d+)?$/.test(
          process.command_line?.[0]?.split("/").pop() ?? process.name,
        ),
      )
      .map(
        (process) =>
          `${process.identity.pid}:${process.identity.start_time_ticks}`,
      ),
  );
  expect(compilers.size).toBeGreaterThan(1);
  const activity = recording.events.filter(
    (event) => event.type === "activity",
  );
  for (const sensor of ["cpu", "files"] as const) {
    expect(
      activity.some((event) =>
        event.data[sensor]?.some(
          (sample) =>
            compilers.has(
              `${sample.process_id.pid}:${sample.process_id.start_time_ticks}`,
            ) &&
            ("runtime_ns" in sample ? sample.runtime_ns : sample.bytes) > 0,
        ),
      ),
    ).toBe(true);
  }
});
function chunks(bytes: Uint8Array, size = 1024) {
  let offset = 0;
  return new ReadableStream<Uint8Array<ArrayBuffer>>({
    pull(controller) {
      if (offset === bytes.length) controller.close();
      else {
        controller.enqueue(
          new Uint8Array(bytes.subarray(offset, offset + size)),
        );
        offset = Math.min(offset + size, bytes.length);
      }
    },
  });
}
test("plain JSON and gzip expand to identical recordings, including split magic bytes", async () => {
  const plain = new TextEncoder().encode(json);
  expect(await readRecording(chunks(plain))).toBe(json);
  for (const size of [1, 1024]) {
    const decoded = await readRecording(chunks(gzipSync(plain), size));
    expect(decoded).toBe(json);
    expect(parseRecording(decoded)).toEqual(parseRecording(json));
  }
  // This is also the stream fetch exposes after HTTP Content-Encoding decoding.
  expect(await readRecording(chunks(plain))).toBe(json);
});
test("truncated gzip and corrupt gzip checksums are rejected", async () => {
  const zipped = gzipSync(json);
  await expect(readRecording(chunks(zipped.subarray(0, -4)))).rejects.toThrow();
  zipped[zipped.length - 8] ^= 0xff;
  await expect(readRecording(chunks(zipped))).rejects.toThrow();
});
test("both input and inflated bytes have a streaming size limit", async () => {
  const text = new TextEncoder().encode("a".repeat(8192));
  for (const bytes of [text, gzipSync(text)])
    await expect(readRecording(chunks(bytes), 4096)).rejects.toThrow("128 MiB");
  expect(await readRecording(chunks(text), text.length)).toBe("a".repeat(8192));
});
test("invalid JSON remains subject to the shared recording parser", async () => {
  for (const invalid of ["{", "{}"])
    expect(() => parseRecording(invalid)).toThrow();
  const text = await readRecording(chunks(gzipSync("{}")));
  expect(() => parseRecording(text)).toThrow("Invalid SPACE recording");
});
