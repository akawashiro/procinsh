# procinsh

Linux x86-64 用の Web ベースのプロセスインスペクタです。

## ビルド

Rust stable、C コンパイラ、clang（BPF backend）、bpftool、libelf 開発ファイル、実行カーネルの BTF が必要です。

```sh
cargo build --release --locked
```

## 起動

```sh
sudo ./target/release/procinsh --listen 127.0.0.1:9090
```

ブラウザで http://127.0.0.1:9090 を開きます。終了は `Ctrl+C` です。

詳細ログを出す場合:

```sh
sudo env RUST_LOG=procinsh=debug ./target/release/procinsh --listen 127.0.0.1:9090
```
