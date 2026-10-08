import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import ts from "typescript";
import { afterEach, expect, test } from "vitest";
import {
  collectDependencies,
  overview,
  renderDot,
} from "../scripts/module-dependencies.js";

const directories: string[] = [];
afterEach(() => {
  for (const directory of directories.splice(0))
    rmSync(directory, { recursive: true, force: true });
});

function fixture(sources: Record<string, string>) {
  const root = mkdtempSync(resolve(tmpdir(), "procinsh-dependencies-"));
  directories.push(root);
  const files = Object.entries(sources).map(([name, content]) => {
    const file = resolve(root, name);
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, content);
    return file;
  });
  return {
    root,
    files,
    options: { moduleResolution: ts.ModuleResolutionKind.Bundler },
  };
}

test("resolve production imports and types, excluding external modules and in-source tests", () => {
  const { root, files, options } = fixture({
    "src/list/app.ts": `
      import { value } from "./data.js";
      import type { Info } from "../shared/types.js";
      export { value } from "./data.js";
      export type { Info } from "../shared/types.js";
      type Inline = import("../shared/inline.js").Inline;
      const lazy = import("../shared/lazy.js");
      import "three";
      // import "../shared/comment.js";
      const text = 'import "../shared/string.js"';
      if (import.meta.vitest) {
        const testOnly = import("../shared/test-only.js");
        const fixture = import("../../tests/fixture.js");
      } else {
        const production = import("../shared/fallback.js");
      }
    `,
    "src/list/data.ts": "export const value = 1;",
    "src/shared/types.ts": "export interface Info {}",
    "src/shared/inline.ts": "export interface Inline {}",
    "src/shared/lazy.ts": "export const lazy = 1;",
    "src/shared/test-only.ts": "export const testOnly = 1;",
    "src/shared/fallback.ts": "export const production = 1;",
    "src/isolated/app.ts": "export const isolated = 1;",
    "tests/fixture.ts": 'import "../src/isolated/app.js";',
  });
  const graph = collectDependencies(files, options, resolve(root, "src"));
  expect(graph.nodes).not.toContain("../tests/fixture.ts");
  expect(graph.edges).toEqual([
    ["list/app.ts", "list/data.ts"],
    ["list/app.ts", "shared/fallback.ts"],
    ["list/app.ts", "shared/inline.ts"],
    ["list/app.ts", "shared/lazy.ts"],
    ["list/app.ts", "shared/types.ts"],
  ]);
  expect(overview(graph)).toEqual({
    nodes: ["isolated", "list", "shared"],
    edges: [["list", "shared"]],
  });
  const dot = renderDot(graph, true);
  expect(dot).toContain('subgraph "cluster_isolated"');
  expect(dot).toContain('"list/app.ts" -> "shared/types.ts";');
  expect(dot).not.toContain('"list/app.ts" -> "shared/test-only.ts";');
});

test("use tsconfig path aliases and retain cycles", () => {
  const { root, files, options } = fixture({
    "src/a/app.ts": 'import "@shared/data.js";',
    "src/shared/data.ts": 'export * from "../a/app.js";',
  });
  const graph = collectDependencies(
    files,
    {
      ...options,
      baseUrl: resolve(root, "src"),
      paths: { "@shared/*": ["shared/*"] },
    },
    resolve(root, "src"),
  );
  expect(graph.edges).toEqual([
    ["a/app.ts", "shared/data.ts"],
    ["shared/data.ts", "a/app.ts"],
  ]);
  expect(overview(graph).edges).toEqual([
    ["a", "shared"],
    ["shared", "a"],
  ]);
});

test("fail on unresolved relative production imports", () => {
  const { root, files, options } = fixture({
    "src/app.ts": 'import "./missing.js";',
  });
  expect(() =>
    collectDependencies(files, options, resolve(root, "src")),
  ).toThrow("Cannot resolve ./missing.js");
});
