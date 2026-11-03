import { defineConfig } from "vitest/config";


export default defineConfig({
  resolve: {
    dedupe: ["three"],
  },
  test: {
    environment: "node",
    include: ["tests/**/*.test.ts"],
    includeSource: ["src/**/*.ts"],
  },
});
