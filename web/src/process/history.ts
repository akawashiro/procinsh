// Draw CPU/RSS history and its axes on a supplied Canvas 2D surface.
import { bytes } from "../shared/display.js";
import type { HistoryPoint } from "../shared/api-types.js";
function historyMemoryLimit(peak: number): number {
  return (
    [1024, 1024 ** 2, 1024 ** 3, 1024 ** 4].find((limit) => peak <= limit) ??
    1024 ** 4
  );
}
export function drawHistory(
  canvas: HTMLCanvasElement,
  legend: HTMLElement,
  points: HistoryPoint[],
  scale = window.devicePixelRatio || 1,
) {
  const ctx = canvas.getContext("2d");
  if (!ctx || !points.length) return;
  const width = Math.max(260, canvas.clientWidth - 32),
    height = 176;
  canvas.width = width * scale;
  canvas.height = height * scale;
  ctx.scale(scale, scale);
  const end = points.at(-1)!.timestamp,
    rssMax = historyMemoryLimit(Math.max(...points.map((p) => p.rss_bytes))),
    left = 48,
    right = width - 76,
    top = 24,
    bottom = 148,
    plotWidth = right - left,
    plotHeight = bottom - top;
  ctx.font = "11px system-ui";
  ctx.textBaseline = "middle";
  ctx.fillStyle = "#66dfc5";
  ctx.textAlign = "right";
  ctx.fillText("CPU", left - 8, 9);
  ctx.fillStyle = "#8ab4ff";
  ctx.textAlign = "left";
  ctx.fillText("RSS", right + 8, 9);
  ctx.lineWidth = 1;
  for (let i = 0; i <= 4; i++) {
    const fraction = i / 4,
      y = bottom - fraction * plotHeight;
    ctx.strokeStyle = "#263246";
    ctx.beginPath();
    ctx.moveTo(left, y);
    ctx.lineTo(right, y);
    ctx.stroke();
    ctx.fillStyle = "#66dfc5";
    ctx.textAlign = "right";
    ctx.fillText(`${i * 25}%`, left - 8, y);
    ctx.fillStyle = "#8ab4ff";
    ctx.textAlign = "left";
    ctx.fillText(bytes(rssMax * fraction), right + 8, y);
    const x = left + fraction * plotWidth;
    ctx.strokeStyle = "#263246";
    ctx.beginPath();
    ctx.moveTo(x, top);
    ctx.lineTo(x, bottom);
    ctx.stroke();
    ctx.fillStyle = "#93a2b9";
    ctx.textAlign = "center";
    ctx.fillText(`${i * 15 - 60}s`, x, bottom + 18);
  }
  ctx.save();
  ctx.beginPath();
  ctx.rect(left, top, plotWidth, plotHeight);
  ctx.clip();
  for (const [key, max, color] of [
    ["cpu_percent", 100, "#66dfc5"],
    ["rss_bytes", rssMax, "#8ab4ff"],
  ] as const) {
    ctx.strokeStyle = color;
    ctx.lineWidth = 2;
    ctx.beginPath();
    let started = false;
    for (const p of points) {
      if (p[key] == null) {
        started = false;
        continue;
      }
      const x = left + (plotWidth * (p.timestamp - end + 60000)) / 60000,
        y = bottom - plotHeight * Math.min(1, Math.max(0, p[key] / max));
      if (started) ctx.lineTo(x, y);
      else ctx.moveTo(x, y);
      started = true;
    }
    ctx.stroke();
  }
  ctx.restore();
  legend.textContent = `CPU 0–100% · RSS 0–${bytes(rssMax)}`;
}

if (import.meta.vitest) {
  const { test } = import.meta.vitest;
  test("history-chart regression", async () => {
    const assert: typeof import("node:assert/strict") = (
      await import("node:assert/strict")
    ).default;
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
      {
        timestamp: 1000,
        cpu_percent: 0,
        vms_bytes: 0,
        rss_bytes: 1024 ** 2 / 2,
      },
      {
        timestamp: 31000,
        cpu_percent: null,
        vms_bytes: 0,
        rss_bytes: 1024 ** 2,
      },
      {
        timestamp: 61000,
        cpu_percent: 250,
        vms_bytes: 0,
        rss_bytes: 1024 ** 2,
      },
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
}
