# ProcInSh - Process in the Shell

A web-based process inspector for Linux.
Like [Ghost in the Shell](https://en.wikipedia.org/wiki/Ghost_in_the_Shell), you can wander through process space with your ghost.

_Now, where shall I go? The process space is vast._

## Screenshot

![Screenshot of space view](./space_screenshot.png)

[screenmovie.webm](https://github.com/user-attachments/assets/7e964874-7e5a-46a9-8bb3-0c33defc9f42)

## Build

Requires Node.js 22 or newer with npm, stable Rust, a C compiler, clang with the BPF backend, bpftool, libelf development files, and BTF information for the running kernel.

```sh
npm ci
npm run build:web
cargo build --release --locked
```

The web UI is written in TypeScript in `src/web/`. `npm run build:web` checks
types and compiles the UI into `dist/web/`, which Cargo embeds into the binary.
Generated JavaScript is not committed. Re-run the web build before Cargo whenever
you edit TypeScript; Cargo does not invoke npm or install dependencies for you.
Node.js and npm are not needed to run the resulting binary.

For web development checks:

```sh
npm run typecheck
npm run build:web
node tests/space-model.mjs
```

API contracts live in `src/web/api-types.ts` and must be kept aligned with the
Rust JSON responses (including nullability and hex-string addresses). They are
compile-time types, not runtime validators. The bundled Three.js r180 runtime
is unchanged; its matching type definitions are build-only dependencies.

Browser regression checks require Chrome, the debug binary, and C fixtures:

```sh
npm run build:web
cargo build --locked
bash tests/targets/build.sh
node tests/browser.mjs
node tests/space-browser.mjs
```

## Run

```sh
sudo ./target/release/procinsh --listen 127.0.0.1:9090
```

Open http://127.0.0.1:9090 in your browser. Press `Ctrl+C` to stop.

To enable debug logging:

```sh
sudo env RUST_LOG=procinsh=debug ./target/release/procinsh --listen 127.0.0.1:9090
```
