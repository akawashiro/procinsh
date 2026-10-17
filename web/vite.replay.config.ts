// Node-only static build: publish dist-replay directly on Cloudflare Pages Free.
import { readdirSync, statSync } from "node:fs";
import { resolve } from "node:path";
import { defineConfig } from "vite";

export const PAGES_MAX_ASSET_BYTES = 25 * 1024 * 1024;
export function assertPagesAssets(directory: string) {
  for (const name of readdirSync(directory)) {
    const path = resolve(directory, name);
    const stat = statSync(path);
    if (stat.isDirectory()) assertPagesAssets(path);
    else if (stat.size > PAGES_MAX_ASSET_BYTES)
      throw new Error(
        `Cloudflare Pages Free asset exceeds 25 MiB: ${path} (${stat.size} bytes)`,
      );
  }
}
export default defineConfig({
  base: process.env.PROCINSH_REPLAY_BASE || "/",
  appType: "mpa",
  define: { "import.meta.vitest": "undefined" },
  publicDir: "replay-public",
  plugins: [
    {
      name: "pages-free-asset-limit",
      buildStart() {
        assertPagesAssets(resolve(import.meta.dirname, "replay-public"));
      },
      closeBundle() {
        assertPagesAssets(resolve(import.meta.dirname, "dist-replay"));
      },
    },
  ],
  build: {
    outDir: "dist-replay",
    rolldownOptions: {
      input: {
        "replay/space": resolve(import.meta.dirname, "replay/space/index.html"),
      },
    },
  },
});
