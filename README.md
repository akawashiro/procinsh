# ProcInSh - Process in the Shell

ProcInSh is a web-based process inspector for Linux.

## Screenshot

![Screenshot of space view](./space_screenshot.png)

[screenmovie.webm](https://github.com/user-attachments/assets/7e964874-7e5a-46a9-8bb3-0c33defc9f42)

## Build

Requires stable Rust, a C compiler, clang with the BPF backend, bpftool, libelf development files, and BTF information for the running kernel.

```sh
cargo build --release --locked
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
