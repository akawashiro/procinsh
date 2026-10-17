import { readFileSync, cpSync, mkdirSync } from "node:fs";
import { resolve } from "node:path";
import { defineConfig, loadEnv, type Plugin } from "vite";

/** Map browser routes to HTML entries without changing the browser's URL. */
function applicationPages(): Plugin {
  const version = readFileSync(
    new URL("../Cargo.toml", import.meta.url),
    "utf8",
  ).match(/^version = "([^"]+)"/m)![1];
  return {
    name: "procinsh-pages",
    closeBundle() {
      const directory = resolve(import.meta.dirname, "dist/recordings");
      mkdirSync(directory, { recursive: true });
      cpSync(
        resolve(import.meta.dirname, "replay-public/recordings"),
        directory,
        { recursive: true },
      );
    },
    configureServer(server) {
      server.middlewares.use((request, response, next) => {
        const url = new URL(request.url ?? "/", "http://localhost");
        const path = url.pathname;
        if (path === "/recordings/demo.json") {
          response.setHeader("content-type", "application/json; charset=utf-8");
          response.end(
            readFileSync(
              resolve(
                import.meta.dirname,
                "replay-public/recordings/demo.json",
              ),
            ),
          );
          return;
        }
        const page = /^\/list\/?$/.test(path)
          ? "list"
          : /^\/process\/\d+\/?$/.test(path)
            ? "process"
            : /^\/(?:space\/?)?$/.test(path)
              ? "space"
              : null;
        if (page) request.url = `/${page}/index.html${url.search}`;
        next();
      });
    },
    transformIndexHtml: {
      handler(html, context) {
        return context.server
          ? html
              .replaceAll("{{PROCINSH_VERSION}}", version)
              .replaceAll("{{PROCINSH_BUILD_REVISION}}", " · development")
          : html;
      },
    },
  };
}

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, import.meta.dirname, "PROCINSH_");
  const backend = env.PROCINSH_BACKEND_URL || "http://127.0.0.1:9090";
  return {
    appType: "mpa",
    define: { "import.meta.vitest": "undefined" },
    resolve: {
      dedupe: ["three"],
    },
    plugins: [applicationPages()],
    build: {
      manifest: true,
      rolldownOptions: {
        input: {
          ...Object.fromEntries(
            ["list", "process", "space", "replay/space"].map((page) => [
              page,
              resolve(import.meta.dirname, page, "index.html"),
            ]),
          ),
        },
      },
    },
    server: {
      proxy: {
        "/api": {
          target: backend,
          changeOrigin: true,
          configure(proxy) {
            proxy.on("proxyReq", (outgoing, incoming) => {
              // Preserve the backend's origin guard for cross-origin requests.
              if (incoming.headers.origin === `http://${incoming.headers.host}`)
                outgoing.setHeader("origin", new URL(backend).origin);
            });
          },
        },
      },
    },
  };
});
