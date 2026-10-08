// Draw CPU/RSS history and its axes on a supplied Canvas 2D surface.
import { bytes } from "../shared/display.js";
import type { HistoryPoint } from "../shared/api-types.js";
export function historyMemoryLimit(peak: number): number {
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
