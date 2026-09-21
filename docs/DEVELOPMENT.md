# ProcInSh 開発ドキュメント

ProcInSh は Linux x86-64 のプロセスを観測する Web アプリケーションです。Rust の HTTP サーバーが `/proc`、ptrace、eBPF から情報を取得し、ブラウザに配信します。プロセスのメモリやレジスタを書き換える機能はありません。ただし、ptrace スナップショットの取得中は対象の全スレッドを一時停止します。

この文書は現在の実装構成、動作、API、開発・検証手順を説明します。以下のコマンドはリポジトリのルートで実行します。

## ビルドと実行環境

Rust edition は 2024 です。Rust は rustup 経由で利用し、`rust-toolchain.toml` でバージョンと rustfmt・clippy を固定しています。ローカルと CI は同じ設定を使い、必要なツールチェーンは rustup が自動インストールします。Rust の更新時はこのファイルを変更し、フォーマット・Clippy・テストを再確認します。ビルドには Node.js 22以降と npm、C コンパイラ、`ar`、BPF backend を持つ clang、bpftool、pkg-config、libelf・zlib 開発ファイル、実行カーネルの `/sys/kernel/btf/vmlinux` が必要です。

`build.rs` は bpftool で BTF から `vmlinux.h` を生成し、`libbpf-cargo` で CPU/IPC とファイル I/O の BPF オブジェクトをビルドします。通常の `cargo build` でもこの処理を実行するため、BPF を画面で利用しない場合もビルド依存は必要です。

```sh
npm ci
npm run build:web
cargo build --locked
sudo ./target/debug/procinsh --listen 127.0.0.1:9090

# リリースビルド
cargo build --release --locked
```

ブラウザで http://127.0.0.1:9090 を開きます。Web UI は `src/web/` の TypeScript で実装しています。`npm run build:web` は型チェックと `dist/web/` への JavaScript 生成を行います。生成物は Git に含めず、Cargo は npm を自動実行しません。生成物がない場合、Cargo のビルドは準備手順を表示して失敗します。

HTML/CSS、生成した JavaScript、Three.js（revision 180）はバイナリに埋め込みます。TypeScript を変更したら `npm run build:web` の後に Rust バイナリを再ビルドしてください。Cargo は TypeScript と生成物の鮮度を検証しません。HTML/CSS の変更にも Rust の再ビルドが必要です。実行時の Node.js・npm、外部 CDN は不要です。SPACE の描画には WebGL2 が必要です。

| CLI オプション | 動作 |
|---|---|
| `--listen ADDRESS` | 待受アドレス。既定は `127.0.0.1:8080` |
| `--pid PID` | 起動時に詳細監視の対象を選択 |
| `--interval DURATION` | 詳細監視の更新間隔。既定は `1s`、範囲は `100ms`～`60s` |
| `--help` / `--version` | ヘルプ / バージョン表示 |

SIGINT（Ctrl+C）または SIGTERM で収集停止と HTTP サーバーの終了処理を行います。起動・サーバーの致命的な失敗は非ゼロ終了です。

## 実装構成

| 場所 | 役割 |
|---|---|
| `src/main.rs` | CLI、ロガー初期化、待受、終了処理 |
| `src/server/` | Axum のルーティング、入力・アクセス検証、HTTP ログ、詳細監視の SSE |
| `src/state/` | 選択対象、定期観測、60秒の履歴、最新状態の配信 |
| `src/process/` | `/proc` の解析、PID 識別、プロセス・スレッド・メモリ・FD・ソケット・シグナル情報 |
| `src/snapshot/` | ptrace による停止・レジスタ取得、frame pointer unwind、逆アセンブル |
| `src/symbol/` | ELF/DWARF によるシンボル・ソース位置の解決 |
| `src/space/` | 全プロセスの構造、閲覧セッション、BPF 収集、名前解決、SPACE API |
| `src/web/` | TypeScript の通常画面・SPACE 画面・描画モデル、HTML/CSS、同梱 Three.js |
| `tests/` | Rust・ブラウザ・ログ・実機センサーのテストと C fixture |

Tokio/Axum が HTTP と SSE を処理し、ブロッキングする詳細 API は `spawn_blocking` に渡します。定期観測と SPACE の収集は OS スレッドで動きます。詳細監視は `AppState`、SPACE は独立した `Space` に状態を保持します。

Web UI の API 型は `src/web/api-types.ts` に定義し、Rust の JSON 応答と合わせて管理します。null の扱いや16進文字列のアドレスも契約に含まれます。これらはコンパイル時の型で、実行時の入力検証ではありません。TypeScript と Three.js の型定義はビルド専用の npm 依存です。

## プロセス詳細の観測

`/` にプロセス一覧、`/process/{pid}` に詳細画面を表示します。対象の識別子は `{pid, start_time_ticks}` です。PID の再利用や選択変更を検出し、別のプロセスの情報と混同しないように検証します。詳細監視の対象はサーバー全体で1つで、複数タブも選択を共有します。

定期観測は CPU、RSS/VMS、fault、I/O、context switch、スレッドなどを収集し、直近60秒の履歴を保持します。CPU 使用率は1コアを100%とし、差分のない初回は N/A です。maps/smaps は選択対象について約5秒間隔で更新します。通常の `/proc` 読み取りは対象を停止しないため、各フィールドの取得時点は厳密には一致しません。

詳細画面で追加取得する情報は次のとおりです。

- メモリ：`process_vm_readv` で読み取り、hex/ASCII 表示。最大64 KiBで、部分読み取りを区別します。
- FD：pipe/FIFO/socket の方向、接続候補、共有所有者を表示。UNIX peer は socket diagnostic、TCP/UDP は対象の network namespace の情報から探索します。データを消費する読み取りは行いません。探索は3秒・100,000 FD・一致8192 FDを上限とし、不完全な結果を区別します。
- 環境変数：`environ` を最大1 MiB読み取り、重複名、空値、値中の `=` を維持します。通常は exec 時の環境領域で、起動後の変更すべてを反映するものではありません。
- 補助ベクトル：ELF の32/64 bitを判別して auxv を最大64 KiB読み取り、既知・未知のタグを表示します。文字列参照は最大4096バイトで、読めなくても数値を保持します。big-endian ELF は対象外です。
- シグナル：プロセスとスレッドの保留・ブロック・無視・ハンドラ登録のマスクを表示します。最大4096スレッド・2秒で打ち切ります。受信履歴や送信元を追跡せず、シグナルを送信する機能はありません。

環境変数、auxv、FD、シグナルのパネルは必要時に取得・再取得し、通常の定期配信とは分けています。文字列は HTML として解釈せず表示します。

### スナップショット、スタック、逆アセンブル

スナップショットは `PTRACE_SEIZE` と `PTRACE_INTERRUPT` で全スレッドの停止を確認してから、レジスタ・マップ・スタック・命令バイトを取得します。追加スレッドを再列挙し、4096スレッド・16回の安定化試行・停止待ち2秒を上限とします。取得にも2秒の処理予算がありますが、カーネル内でブロックする syscall の実時間を保証するものではありません。

RAII と専用 OS スレッドの終了で detach を扱い、既存の job-control stop と signal delivery を維持します。自分自身のスナップショットは拒否します。シンボル解決と命令デコードは対象の再開後に行います。

スタックは RBP をたどる最大256フレームの unwind です。ELF/DWARF から PIE/ASLR を考慮して関数、行、inline frame を解決し、ファイルの device/inode/size/mtime でキャッシュします。解決できない場合は生アドレスを表示します。frame pointer のないコードや signal trampoline を含む任意のスタックを完全に復元するものではありません。

```sh
# 観測対象の C/C++ プログラム
cc -g -fno-omit-frame-pointer -fno-optimize-sibling-calls target.c -o target

# 観測対象の Rust プロジェクトで実行
RUSTFLAGS="-C force-frame-pointers=yes" cargo build
```

逆アセンブルは停止中の RIP から最大256バイトを取得し、`iced-x86` で最大32命令を Intel 構文で表示します。実メモリを使うため JIT のコードも対象ですが、32-bit compatibility mode は対象外です。Memory Viewer はスナップショット保存値ではなく、要求時点のメモリを読みます。

画面の自動スナップショットは既定 OFF、ON にすると1秒間隔で要求します。処理を重複させず、対象変更、タブ非表示、対象終了、取得失敗で停止します。毎回対象を一時停止する点は手動取得と同じです。

## SPACE の観測

`/space` はプロセスの親子関係、仮想アドレス空間、pipe/socket の接続、ネットワーク接続先、ファイル I/O を3D表示します。プロセス内のアドレスの隙間を圧縮し、高さを正規化するため、プロセス間の同じ高さは同じアドレスを意味しません。

構造の収集は約5秒、CPU/RSS の更新は約1秒、活動集計の配信は最大10Hzです。全体 FD 走査は100,000 FD・4秒、各 PID のマッピングは4096件、接続図は約20,000接続を上限とします。探索は非停止で、取得不能・打ち切り・欠落を状態として扱います。

| センサー | 観測内容と制約 |
|---|---|
| CPU | CO-RE eBPF の `sched_switch` で実行時間と実行中 CPU を集計。描画は実行中に発光し、終了後約500msで減衰 |
| IPC | pipe read/write と socket の送受信結果を観測。ペイロードは読まず、MSG_PEEK は加算しない。splice/sendfile、一部 io_uring、帰属不明のワーカーは対象外 |
| ファイル I/O | 独立した BPF で VFS の read/write、ベクトル I/O の成功バイト数と回数を観測。ページキャッシュ経由も含む。mmap、io_uring、splice/sendfile、物理ディスク転送量は対象外 |

ファイルのパスは操作時に取得し、取得できない場合は device/inode 等の識別子を使います。画面は最終アクセスから30秒、各プロセス32個・全体512個まで保持します。IPC の共有 FD や複数所有者は一意な通信相手と区別します。

閲覧は lease で管理します。ブラウザは10秒ごとに更新し、期限は30秒、最大32セッションです。タブ非表示やページ離脱で解放し、最後の lease がなくなるとセンサーを解放して収集を休止します。ワーカースレッドはアプリ終了まで残ります。グラフ上の選択はブラウザ内だけで管理し、収集対象や通常画面の選択には影響しません。

BPF のフックが利用できない場合はセンサーごとの理由を状態 API とログに出し、利用可能な情報の収集を継続します。必要なカーネル機能・権限はセンサーごとに異なります。

## HTTP API

JSON のプロセス識別子は `{ "pid": 123, "start_time_ticks": 456 }` です。アドレスは JavaScript の整数精度を保つため16進文字列で返します。

| Method / path | 内容 |
|---|---|
| `GET /api/config` | バージョン、更新間隔、履歴秒数 |
| `GET /api/processes` | プロセス一覧 |
| `GET /api/target` | 選択対象、最新観測、履歴、マップ。未選択は null |
| `POST /api/target` | 識別子の JSON で対象を選択 |
| `DELETE /api/target` | 識別子の JSON で対象を解除 |
| `GET /api/target/process` | 最新のプロセス観測 |
| `GET /api/target/threads` | スレッド観測 |
| `GET /api/target/maps` | maps/smaps、rollup、取得時刻 |
| `GET /api/target/memory` | `address` と `length` で指定するメモリ |
| `GET /api/target/fds` | FD、接続候補、共有所有者 |
| `GET /api/target/environment` | 環境変数 |
| `GET /api/target/auxv` | 補助ベクトル |
| `GET /api/target/signals` | プロセス・スレッドのシグナル情報 |
| `POST /api/target/snapshot` | 識別子の JSON でスナップショット取得 |
| `GET /api/target/events` | SSE の `observation` イベント |
| `GET /api/space/status` | センサー状態と収集統計 |
| `GET /api/space/snapshot` | 最新の構造 |
| `POST /api/space/leases` | 閲覧開始・更新 |
| `DELETE /api/space/leases` | `{ "token": "..." }` で閲覧終了 |
| `GET /api/space/events?token=TOKEN` | SPACE の SSE |

詳細 GET（process/threads/maps/memory/fds/environment/auxv/signals）には `pid` と `start_time_ticks` のクエリが必要です。対象未選択は404、選択不一致は409、対象終了は410、無効なメモリ範囲は400で返します。

詳細 SSE は最新状態を watch channel で配信し、接続時にも現在の値を送ります。SPACE は `topology`、`metrics`、`activity` を配信します。購読側が遅延した場合は `gap` と現在の構造を送り、古い活動を再生しません。

lease の JSON は省略可能な `token` だけを受け付け、未知のフィールドは拒否します。`{}` で新規作成、`{"token":"..."}` で既存セッションを更新します。応答は `token`、`expires_in` です。

`activity.files` は `{process_id, resource, path, write, bytes, count}` の配列です。`path` は取得不能なら null、`resource` は device/inode/generation を含む識別子です。状態には `files`、`files_lost`、`files_coverage` などを含みます。

## 権限とログ

`ptrace` と `process_vm_readv` は所有者、dumpable 属性、Yama、`CAP_SYS_PTRACE`、seccomp などの制約を受けます。BPF はカーネル側の対応と観測権限も必要です。権限やカーネル設定の自動変更、sudo の自動実行はしません。

待受は既定で loopback です。Host/Origin/Fetch Metadata を検証し、API レスポンスに `Cache-Control: no-store` を付けます。認証機能はありません。外部アドレスで待ち受けると警告を出すため、公開範囲を管理する必要があります。

`deploy/procinsh-capabilities.conf` は systemd の `[Service]` 用断片で、`CAP_SYS_PTRACE CAP_DAC_READ_SEARCH CAP_BPF CAP_PERFMON` を AmbientCapabilities と CapabilityBoundingSet に指定しています。完全な service unit は同梱していません。

ログは `log` と `env_logger` を使い、標準エラーに時刻・レベル・モジュール名を出します。既定は `info` です。

- `info`：起動・終了、対象の選択・解除・終了、収集状態と復旧。
- `warn`：観測失敗、センサー利用不可。同じ状態・エラーの連続出力を抑制。
- `error`：致命的な実行失敗、ワーカー異常、HTTP 500系。
- `debug`：HTTP のメソッド・パス・ステータス・応答生成時間、API エラー詳細、構造収集件数。

```sh
sudo env RUST_LOG=procinsh=debug ./target/debug/procinsh --listen 127.0.0.1:9090
sudo env RUST_LOG=info,procinsh::space=debug ./target/debug/procinsh --listen 127.0.0.1:9090
```

`RUST_LOG=off` はアプリケーションのログを抑制します。HTTP アクセスログにはクエリ、トークン、本文を含めず、観測したメモリや環境変数の値も記録しません。SSE の応答時間は接続開始時の応答までです。

## 検証

ビルドと fixture の準備後に実行します。Rust 結合テストの一部も fixture をビルドしますが、`tests/space.rs` の単独実行には事前準備が必要です。

```sh
npm ci
npm run typecheck
npm run build:web
cargo build --locked
sh tests/targets/build.sh
cargo test --locked
python3 tests/logging-checks.py
node tests/space-model.mjs

# フォーマット・静的解析
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
```

CI はフォーマット確認、TypeScript の型チェックとビルド、Rust の全ターゲットのビルド、Clippy、Rust テスト、ログ検証、SPACE モデル検証を実行します。ブラウザと実機センサーのテストは別途実行します。

Rust テストは `/proc` の解析、PID 再利用、メモリ読み取り、ptrace の解除、シンボル、HTTP、SPACE の構造・lease・集計を検証します。ログテストは既定レベル、debug、off、標準エラー、SIGTERM、ポート競合、クエリ非出力を検証します。プロセス観測を拒否するサンドボックスでは一部テストが失敗するため、テスト対象への ptrace/process_vm_readv とローカル通信が許可された環境が必要です。

### ブラウザテスト

上記の Web・Rust ビルドと fixture の準備に加え、Google Chrome または Chromium が必要です。テスト自体は Node.js 標準機能と DevTools Protocol を使い、追加の npm テストライブラリは不要です。

```sh
node tests/browser.mjs
node tests/space-browser.mjs

# ブラウザのパスを指定する場合
CHROME=/usr/bin/chromium node tests/browser.mjs
CHROME=/usr/bin/chromium node tests/space-browser.mjs
```

通常画面は検索・選択・SSE・スナップショット・詳細パネル・終了処理を、SPACE は WebGL、配置、選択、ネットワークとファイルの描画を検証します。ブラウザテストは一時サーバーとブラウザプロファイルを作り、終了時に片付けます。画面・モデルの検証と実機センサーの検証は別です。

### 実機センサーテスト

BPF の観測権限（`CAP_BPF`・`CAP_PERFMON` など）と対応するカーネルのフックを利用できるサーバーに対して実行します。CPU/IPC とファイル I/O を検証します。センサー利用不可を成功扱いにはしません。

```sh
python3 tests/space-live.py http://127.0.0.1:9090
python3 tests/space-files-live.py http://127.0.0.1:9090
```

URL を省略した場合は一時サーバーを起動・終了しますが、そのサーバーにも観測権限が必要です。

`tests/targets/` の C fixture には計算、sleep、スレッド増減、メモリ確保、再帰、mmap、IPC、活動計測用のプログラムがあります。手動観測には次を使えます。

```sh
tests/targets/bin/recursive --allow-inspector
```

`--allow-inspector` は当該 fixture の ptrace 許可を設定するテスト専用オプションです。システム全体の Yama 設定は変更しません。
