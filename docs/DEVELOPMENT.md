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

詳細 GET（process/threads/maps/memory/fds/environment/auxv/signals）には `pid` と `start_time_ticks` のクエリが必要です。対象未選択は404、選択不一致は409です。要求時に対象の生存を確認するAPIでは終了・PID再利用を410で返しますが、保持済みの process/threads/maps は終了後も取得できます。memory の `address` は10進または `0x` 付き16進、`length` は既定256・範囲1～65536です。不正なアドレス・範囲は400、その他の観測処理の失敗は原則422、ブロッキングタスクの失敗は500です。

詳細 SSE の `observation` は対象の最新状態または null です。SPACE の SSE は `topology`（構造）、`metrics`（CPU/RSS）、`activity`（活動集計）、`gap`（欠落フレーム数）を配信します。いずれも接続時に初期状態を送ります。

lease の JSON は省略可能な `token` だけを受け付け、未知のフィールドは拒否します。`{}` で新規作成、`{"token":"..."}` で既存セッションを更新します。応答は `token`、`expires_in`（30秒）です。期限切れ・未知のトークンでの更新やセッション数上限超過は422です。DELETE は未知のトークンでも `{"ok":true}` を返します。SPACE SSE は登録されていないトークンを403で拒否します。

`activity.files` は `{process_id, resource, path, write, bytes, count}` の配列です。`path` は取得不能なら null、`resource` は device/inode/generation を含む識別子です。状態には `files`、`files_lost`、`files_coverage` などを含みます。

## バックエンド側の処理

### 共通処理と状態管理

Axum がルートごとにクエリや JSON を取り出し、ハンドラへ渡します。Host・Origin・Fetch Metadata の検証を通過した応答には no-store とセキュリティヘッダを付けます。ブロッキングする通常画面向けAPIは `spawn_blocking` で実行し、`AppState` 内の状態は mutex で保護します。SPACE は独立した `Space` の状態を参照します。

対象は PID 単独ではなく `{pid, start_time_ticks}` で識別します。選択中の識別子との照合と、実際のプロセスが生存しているかの確認は別の処理です。保持済みデータを返すAPIは前者のみ、メモリなどを追加取得するAPIは後者も行います。詳細監視の選択対象はサーバー全体で1つです。

### 設定とプロセス一覧

- `GET /api/config`：パッケージのバージョン、起動時に指定した更新間隔、履歴の保持秒数を返します。
- `GET /api/processes`：要求ごとに `Discovery` が `/proc` を走査し、プロセス識別子、名前、コマンド、ユーザー、メモリ量などを返します。前回の収集値との差分から CPU 使用率を求めます。初回など差分がない場合は null です。

### 対象の取得・選択・解除

- `GET /api/target`：保持している対象の概要・観測・履歴・マップ・終了状態を返します。未選択なら null です。この要求自体では再観測しません。
- `POST /api/target`：受け取った識別子を検証し、概要・初回観測・maps/smaps を取得します。履歴を初期化し、識別子を再確認して選択状態を置き換え、SSE に公開して新しい対象を返します。
- `DELETE /api/target`：選択中の識別子との一致を確認して対象を解除し、SSE に null を公開します。応答も null です。終了済みの対象も解除できます。

定期観測の OS スレッドは起動時の更新間隔に従い、CPU、RSS/VMS、fault、I/O、context switch、スレッドなどを収集し、直近60秒の履歴を保持します。CPU 使用率は1コアを100%とします。maps/smaps は約5秒間隔で更新します。通常の `/proc` 読み取りは対象を停止せず、各項目の取得時点は厳密には一致しません。終了や観測エラーも対象状態に反映して配信します。

### 保持済みの観測・スレッド・マップ

次のAPIは選択識別子を照合した後、保持済みの値を取り出します。HTTP要求ごとに `/proc` を読み直したり、ptrace で停止したりはしません。

- `GET /api/target/process`：最新の observation を返します。
- `GET /api/target/threads`：最新 observation の threads を返します。
- `GET /api/target/maps`：maps、rollup、取得時刻、取得エラーと process_id を返します。

### メモリ読み取り

`GET /api/target/memory` はアドレスの構文、長さ、加算のオーバーフローを検証し、選択識別子と生存を確認して `process_vm_readv` で最大64 KiBを読み取ります。部分読み取りを完全な読み取りと区別して返します。スナップショットの保存値ではなく、要求時点のメモリを対象を停止せずに取得します。

### FD と接続先

`GET /api/target/fds` は選択・生存を確認し、pipe/FIFO/socket の FD、アクセス方向、接続候補、同じリソースの共有所有者を収集します。UNIX peer は socket diagnostic、TCP/UDP は対象の network namespace の情報から探索します。共有所有者と通信相手は区別し、データを消費する読み取りは行いません。探索は3秒・100,000 FD・一致8192 FDを上限とし、打ち切りなどを結果に含めます。

### 環境変数・補助ベクトル・シグナル

いずれも選択・生存を確認して要求時に取得します。定期観測に含めて再収集するものではありません。

- `GET /api/target/environment`：`environ` を最大1 MiB読み取り、重複名・空値・値中の `=` を維持します。通常は exec 時の環境領域であり、起動後の変更すべてを反映しません。
- `GET /api/target/auxv`：ELF の32/64 bitを判別し、auxv を最大64 KiB読み取ります。既知・未知のタグを扱い、文字列参照は最大4096バイトまで解決します。参照先が読めなくても数値は保持します。big-endian ELF は対象外です。
- `GET /api/target/signals`：プロセスとスレッドの保留・ブロック・無視・ハンドラ登録のマスクを収集します。最大4096スレッド・2秒で打ち切ります。受信履歴や送信元の追跡、シグナル送信は行いません。

### スナップショット

`POST /api/target/snapshot` は選択・生存を確認してスナップショット処理を呼び出します。`PTRACE_SEIZE` と `PTRACE_INTERRUPT` で全スレッドの停止を確認し、レジスタ・マップ・スタック・命令バイトを取得します。追加スレッドを再列挙し、4096スレッド・16回の安定化試行・停止待ち2秒を上限とします。取得にも2秒の処理予算がありますが、カーネル内でブロックする syscall の実時間を保証するものではありません。

RAII と専用 OS スレッドの終了で detach を扱い、既存の job-control stop と signal delivery を維持します。自分自身のスナップショットは拒否します。対象の再開後にシンボル解決と命令デコードを行い、スレッドごとの結果を返します。

スタックは RBP をたどる最大256フレームの unwind です。ELF/DWARF から PIE/ASLR を考慮して関数・行・inline frame を解決し、ファイルの device/inode/size/mtime でキャッシュします。解決できない場合は生アドレスを保持します。frame pointer のないコードや signal trampoline を含む任意のスタックを完全には復元できません。

```sh
# 観測対象の C/C++ プログラム
cc -g -fno-omit-frame-pointer -fno-optimize-sibling-calls target.c -o target

# 観測対象の Rust プロジェクトで実行
RUSTFLAGS="-C force-frame-pointers=yes" cargo build
```

逆アセンブルは停止中の RIP から最大256バイトを取得し、`iced-x86` で最大32命令を Intel 構文にデコードします。実メモリを使うため JIT のコードも対象ですが、32-bit compatibility mode は対象外です。

### 詳細監視の SSE

`GET /api/target/events` は watch channel を購読し、接続時の最新状態を `observation` として送った後、選択・解除・定期観測による変更を配信します。遅い購読者向けに古い状態を蓄積せず、最新値を送ります。10秒間隔の keep-alive を設定し、アプリ終了時には配信を終了します。

### SPACE の状態・構造と閲覧セッション

- `GET /api/space/status`：保持しているセンサー状態・収集統計を返します。
- `GET /api/space/snapshot`：保持している最新の構造を返します。status と snapshot のGET自体は収集を開始しません。
- `POST /api/space/leases`：期限切れのセッションを除き、新規トークンを発行するか既存セッションの更新時刻を更新します。期限は30秒、最大32セッションです。初回には収集ワーカーを起動します。
- `DELETE /api/space/leases`：指定トークンを削除します。最後の lease がなくなるとワーカーが休止状態に入り、センサーを解放します。ワーカースレッドはアプリ終了まで残ります。

構造収集は約5秒、CPU/RSS の更新は約1秒です。全体 FD 走査は100,000 FD・4秒、各 PID のマッピングは4096件、接続図は約20,000接続を上限とします。プロセスの親子・マップ・pipe/socket・ネットワーク接続先を非停止で探索し、取得不能・打ち切り・欠落を状態として扱います。ネットワーク接続先の名前解決結果はキャッシュします。

### SPACE の活動収集と SSE

`GET /api/space/events?token=TOKEN` は登録されたトークンを確認し、broadcast channel を購読します。最初に保持済みの構造を `topology` として返し、その後は構造・メトリクス・活動を配信します。トークン削除やアプリ終了を検出すると接続を終了します。

購読側が遅延した場合は `gap` と最新の `topology` を送り、失われた活動を再生しません。keep-alive は10秒間隔です。活動は最大10Hzで集計・配信します。

| センサー | バックエンドの観測内容と制約 |
|---|---|
| CPU | CO-RE eBPF の `sched_switch` で実行時間と実行中 CPU を集計 |
| IPC | pipe read/write と socket の送受信結果を観測。ペイロードは読まず、MSG_PEEK は加算しない。splice/sendfile、一部 io_uring、帰属不明のワーカーは対象外 |
| ファイル I/O | 独立した BPF で VFS の read/write、ベクトル I/O の成功バイト数と回数を観測。ページキャッシュ経由も含む。mmap、io_uring、splice/sendfile、物理ディスク転送量は対象外 |

ファイルのパスは操作時に取得し、取得できない場合は device/inode 等の識別子を使います。BPF のフックが利用できない場合はセンサーごとの理由を状態 API とログに出し、利用可能な情報の収集を継続します。必要なカーネル機能・権限はセンサーごとに異なります。

## フロントエンド側の処理

### プロセス一覧 `/`

一覧と詳細は同じ HTML と `app.ts` を使い、受信した選択状態に応じて表示と URL を切り替えます。サーバーに選択中の対象があれば、`/` へのアクセスでも詳細表示になります。選択変更は別タブにも SSE 経由で反映されます。

| 利用API | 呼び出すタイミングと用途 |
|---|---|
| `GET /api/config` | 初期化時に更新間隔を取得 |
| `GET /api/target` | 初期化時に共有の選択状態を取得 |
| `GET /api/target/events` | 初期化後に購読し、選択・解除・観測を反映 |
| `GET /api/processes` | 一覧表示時と定期更新時に一覧を取得 |
| `POST /api/target` | 行の選択時に識別子を送信 |

一覧の更新間隔は設定値と1000msの大きい方で、詳細表示中と取得中には一覧の更新を行いません。検索と CPU・RSS・PID による並べ替えは取得済みデータに対してブラウザ内で行います。選択に成功すると詳細表示へ切り替え、`history.replaceState` で URL を更新します。一覧のタイトルは `procinsh / list`、Open Graph View は `/space` へのリンクです。

### プロセス詳細 `/process/{pid}`

| 利用API | 呼び出すタイミングと用途 |
|---|---|
| `GET /api/config`、`GET /api/target` | 一覧と共通の初期化 |
| `GET /api/processes`、`POST /api/target` | 直接アクセスした PID と現在の対象が異なる場合、一覧から識別子を解決して選択 |
| `GET /api/target/events` | 概要・観測・履歴・スレッド・マップ・終了状態を更新 |
| `GET /api/target/environment`、`GET /api/target/auxv`、`GET /api/target/fds`、`GET /api/target/signals` | 各パネルを初めて開くときと再取得操作時 |
| `GET /api/target/memory` | メモリフォーム送信、マップやレジスタなどのアドレス操作時 |
| `POST /api/target/snapshot` | 手動取得と自動取得時 |
| `DELETE /api/target` | Back to process list で選択を解除 |

直接アクセスした PID が一覧にない場合は終了エラーを表示します。画面タイトルは `procinsh / <process name>` です。通常の更新には `observation` イベントを使い、`GET /api/target/process`・`threads`・`maps` は直接呼びません。

受信した履歴から CPU/RSS のグラフを描画し、スレッドやマップを表示します。スナップショットの応答は通常観測とは別に保持し、選択したスレッドのレジスタ・スタック・逆アセンブルを表示します。メモリ応答は hex/ASCII に整形します。環境変数などの文字列は HTML として解釈せず表示し、検索は取得済みデータを使います。

自動スナップショットは既定 OFF で、ON にすると1秒間隔で要求します。取得の重複を避け、自動取得中は手動ボタンを無効化します。対象変更・タブ非表示・ページ離脱・対象終了・取得失敗・SSE切断で停止します。毎回対象を一時停止するAPIである点は手動取得と同じです。

対象変更時には保持した詳細情報をクリアします。追加パネルとスナップショットでは、識別子と要求世代を確認して古い応答を破棄します。追加パネルの再取得が失敗した場合は、以前の結果があれば残したうえでエラーを表示します。

### グラフ `/space`

| 利用API | 呼び出すタイミングと用途 |
|---|---|
| `POST /api/space/leases` | 表示開始時に `{}` で作成し、取得直後と以後10秒ごとにトークンで更新 |
| `GET /api/space/events?token=TOKEN` | lease 取得後に購読し、構造・メトリクス・活動を受信 |
| `DELETE /api/space/leases` | 非表示・離脱・セッション再作成時に解放 |

初期構造も SSE から取得するため、`GET /api/space/status` と `GET /api/space/snapshot` は直接呼びません。通常画面の target API も呼びません。グラフ上の選択はブラウザ内だけで管理し、詳細へのリンクは `/process/{pid}` に移動します。

Three.js でプロセスの親子関係、仮想アドレス空間、接続先、ファイル I/O を3D表示します。マップのアドレスの隙間を圧縮し、高さを正規化するため、プロセス間の同じ高さは同じアドレスを意味しません。検索、選択、カメラ操作、再配置もブラウザ内の処理です。タイトルは `procinsh / graph` です。

- `topology`：構造を更新し、プロセスと接続先の配置を維持しながら追加・削除を反映します。
- `metrics`：各プロセスの CPU/RSS と選択中の詳細を更新します。
- `activity`：CPU の発光、IPC・ネットワークの流れ、ファイル I/O の表示を更新します。CPU の発光は実行中に強まり、活動が途絶えると約500msで減衰します。ファイル表示は最終アクセスから30秒、各プロセス32個・全体512個まで保持します。
- `gap`：描画中の粒子をクリアし、続く構造イベントを反映します。

タブ非表示・ページ離脱時は SSE と更新タイマーを止め、lease を解放して活動表示をクリアします。再表示時は lease を作り直します。作成失敗はエラーを表示して3秒後に再試行し、更新失敗は現在のセッションを停止して再作成します。SSE の再接続は EventSource に任せます。

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
