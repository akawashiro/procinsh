import { test } from "vitest";
import assert from "node:assert/strict";
import { historyMemoryLimit, drawHistory } from "../src/process/history.js";

test("history-chart regression", async () => {
  // Exercise the chart with a recording canvas.

  const labels: unknown[][] = [],
    paths: unknown[][] = [];
  const ctx = new Proxy(
    {},
    {
      get:
        (_, name) =>
        (...args: unknown[]) => {
          if (name === "fillText") labels.push(args);
          if (name === "moveTo" || name === "lineTo")
            paths.push([name, ...args]);
        },
    },
  );
  const canvas = {
    clientWidth: 600,
    width: 0,
    height: 0,
    getContext: () => ctx,
  };
  const legend = { textContent: "" };
  const points = [
    { timestamp: 1000, cpu_percent: 0, vms_bytes: 0, rss_bytes: 1024 ** 2 / 2 },
    { timestamp: 31000, cpu_percent: null, vms_bytes: 0, rss_bytes: 1024 ** 2 },
    { timestamp: 61000, cpu_percent: 250, vms_bytes: 0, rss_bytes: 1024 ** 2 },
  ];
  for (const limit of [1024, 1024 ** 2, 1024 ** 3, 1024 ** 4]) {
    assert.equal(historyMemoryLimit(limit), limit);
    assert.equal(
      historyMemoryLimit(limit + 1),
      Math.min(limit * 1024, 1024 ** 4),
    );
  }
  assert.equal(historyMemoryLimit(0), 1024);
  drawHistory(
    canvas as unknown as HTMLCanvasElement,
    legend as HTMLElement,
    points,
    2,
  );
  assert.equal(legend.textContent, "CPU 0–100% · RSS 0–1 MiB");
  for (const label of [
    "CPU",
    "RSS",
    "0%",
    "25%",
    "50%",
    "75%",
    "100%",
    "0 B",
    "256 KiB",
    "512 KiB",
    "768 KiB",
    "1 MiB",
    "-60s",
    "-45s",
    "-30s",
    "-15s",
    "0s",
  ]) {
    assert.ok(
      labels.some(([text]) => text === label),
      `Missing axis label: ${label}`,
    );
  }
  // CPU values above 100% stay at the top of the plot; null samples break the line.
  assert.deepEqual(paths.slice(20, 22), [
    ["moveTo", 48, 148],
    ["moveTo", 492, 24],
  ]);
  assert.equal(canvas.width, 1136);
  assert.equal(canvas.height, 352);
  console.log(
    "History chart: scale boundaries, axes, CPU cap, sample gaps and pixel scaling passed.",
  );
});
