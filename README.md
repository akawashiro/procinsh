# ProcInSh - Process in the Shell

A web-based process inspector for Linux x86-64.
![Screenshot of space view](./space_screenshot.png)

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
