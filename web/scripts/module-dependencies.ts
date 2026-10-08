/** Generate internal TypeScript dependency graphs from the compiler's AST and resolver. */
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, relative, resolve, sep } from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";
import ts from "typescript";

export interface DependencyGraph {
  nodes: string[];
  edges: [string, string][];
}

function isVitestGuard(expression: ts.Expression): boolean {
  return (
    ts.isPropertyAccessExpression(expression) &&
    expression.name.text === "vitest" &&
    ts.isMetaProperty(expression.expression) &&
    expression.expression.keywordToken === ts.SyntaxKind.ImportKeyword &&
    expression.expression.name.text === "meta"
  );
}

/** Include value/type imports, re-exports and literal dynamic imports; skip in-source tests. */
export function collectDependencies(
  files: string[],
  options: ts.CompilerOptions,
  sourceRoot: string,
): DependencyGraph {
  const names = new Map(
    files
      .filter((file) => {
        const path = relative(sourceRoot, file);
        return !path.startsWith(`..${sep}`) && path !== "..";
      })
      .map((file) => [
        resolve(file),
        relative(sourceRoot, file).split(sep).join("/"),
      ]),
  );
  const edges = new Map<string, [string, string]>();
  const cache = ts.createModuleResolutionCache(
    sourceRoot,
    (path) => path,
    options,
  );
  for (const [file, source] of names) {
    const ast = ts.createSourceFile(
      file,
      readFileSync(file, "utf8"),
      ts.ScriptTarget.Latest,
      true,
    );
    function addDependency(specifier: ts.Expression | undefined): void {
      if (!specifier || !ts.isStringLiteralLike(specifier)) return;
      const module = ts.resolveModuleName(
        specifier.text,
        file,
        options,
        ts.sys,
        cache,
      ).resolvedModule;
      if (!module) {
        if (specifier.text.startsWith(".")) {
          throw new Error(`Cannot resolve ${specifier.text} from ${file}`);
        }
        return;
      }
      const target = names.get(resolve(module.resolvedFileName));
      if (target && target !== source) {
        const edge: [string, string] = [source, target];
        edges.set(JSON.stringify(edge), edge);
      }
    }
    function visit(node: ts.Node): void {
      if (ts.isIfStatement(node) && isVitestGuard(node.expression)) {
        if (node.elseStatement) visit(node.elseStatement);
        return;
      }
      if (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) {
        addDependency(node.moduleSpecifier);
      } else if (
        ts.isImportTypeNode(node) &&
        ts.isLiteralTypeNode(node.argument)
      ) {
        addDependency(node.argument.literal);
      } else if (
        ts.isCallExpression(node) &&
        node.expression.kind === ts.SyntaxKind.ImportKeyword
      ) {
        addDependency(node.arguments[0]);
      } else if (
        ts.isImportEqualsDeclaration(node) &&
        ts.isExternalModuleReference(node.moduleReference)
      ) {
        addDependency(node.moduleReference.expression);
      }
      ts.forEachChild(node, visit);
    }
    visit(ast);
  }
  return {
    nodes: [...names.values()].sort(),
    edges: [...edges.values()].sort(
      ([a, b], [c, d]) => a.localeCompare(c) || b.localeCompare(d),
    ),
  };
}

/** Collapse files into their top-level source directories, keeping isolated groups. */
export function overview(graph: DependencyGraph): DependencyGraph {
  const group = (name: string): string => name.split("/")[0];
  const edges = new Map<string, [string, string]>();
  for (const [source, target] of graph.edges) {
    const edge: [string, string] = [group(source), group(target)];
    if (edge[0] !== edge[1]) edges.set(JSON.stringify(edge), edge);
  }
  return {
    nodes: [...new Set(graph.nodes.map(group))].sort(),
    edges: [...edges.values()],
  };
}

/** Render Graphviz DOT, grouping the detailed graph by source directory. */
export function renderDot(graph: DependencyGraph, nested = false): string {
  const lines = [
    "digraph {",
    '  graph [rankdir=LR, bgcolor="white", pad=0.3];',
    '  node [shape=box, style="rounded,filled", fillcolor="#e8f2ff", fontname="sans-serif"];',
    '  edge [color="#475569", arrowsize=0.8];',
  ];
  const quote = JSON.stringify;
  const emit = (name: string, indent: string): void => {
    const label = nested ? name.slice(name.lastIndexOf("/") + 1) : name;
    lines.push(
      `${indent}${quote(name)} [label=${quote(label)}, tooltip=${quote(name)}];`,
    );
  };
  if (nested) {
    const groups = new Map<string, string[]>();
    for (const name of graph.nodes) {
      const directory = name.includes("/") ? name.split("/")[0] : "";
      const nodes = groups.get(directory) ?? [];
      nodes.push(name);
      groups.set(directory, nodes);
    }
    for (const [directory, nodes] of groups) {
      if (!directory) {
        nodes.forEach((name) => emit(name, "  "));
        continue;
      }
      lines.push(`  subgraph ${quote(`cluster_${directory}`)} {`);
      lines.push(
        `    graph [label=${quote(directory)}, fontname="sans-serif", color="#94a3b8", style="rounded", margin=16, labeljust=l];`,
      );
      nodes.forEach((name) => emit(name, "    "));
      lines.push("  }");
    }
  } else {
    graph.nodes.forEach((name) => emit(name, "  "));
  }
  for (const [source, target] of graph.edges) {
    lines.push(`  ${quote(source)} -> ${quote(target)};`);
  }
  return [...lines, "}", ""].join("\n");
}

function main(): void {
  const webRoot = fileURLToPath(new URL("../", import.meta.url));
  const config = ts.readConfigFile(
    resolve(webRoot, "tsconfig.json"),
    ts.sys.readFile,
  );
  const parsed = ts.parseJsonConfigFileContent(config.config, ts.sys, webRoot);
  const diagnostics = [
    ...(config.error ? [config.error] : []),
    ...parsed.errors,
  ];
  if (diagnostics.length) {
    throw new Error(
      ts.formatDiagnosticsWithColorAndContext(diagnostics, {
        getCanonicalFileName: (file) => file,
        getCurrentDirectory: () => webRoot,
        getNewLine: () => "\n",
      }),
    );
  }
  const graph = collectDependencies(
    parsed.fileNames,
    parsed.options,
    resolve(webRoot, "src"),
  );
  if (!graph.nodes.length)
    throw new Error("No TypeScript source modules found");
  const output = resolve(webRoot, "../target/doc/typescript-architecture");
  mkdirSync(output, { recursive: true });
  for (const [name, data, nested] of [
    ["module-dependencies", overview(graph), false],
    ["module-dependencies-detail", graph, true],
  ] as const) {
    const dot = resolve(output, `${name}.dot`);
    writeFileSync(dot, renderDot(data, nested));
    execFileSync("dot", ["-Tsvg", dot, "-o", resolve(output, `${name}.svg`)], {
      stdio: "inherit",
    });
  }
  copyFileSync(
    resolve(dirname(fileURLToPath(import.meta.url)), "architecture.html"),
    resolve(output, "index.html"),
  );
  console.log(`TypeScript dependency graphs: ${output}/index.html`);
}

if (
  process.argv[1] &&
  pathToFileURL(resolve(process.argv[1])).href === import.meta.url
)
  main();
