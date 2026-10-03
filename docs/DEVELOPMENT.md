# ProcInSh 開発ドキュメント

ProcInSh は Linux x86-64 のプロセスを観測する Web アプリケーションです。Rust の HTTP サーバーが `/proc`、perf、eBPF から情報を取得し、ブラウザに配信します。

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

開発用 checkout と独立した最新 main を LAN・VPN から常時見る場合は、[main の常設プレビュー](MAIN_PREVIEW.md)を参照してください。systemd サービスと約10秒間隔の自動更新を利用できます。

HTML/CSS、生成した JavaScript、Three.js（revision 180）はバイナリに埋め込みます。TypeScript を変更したら `npm run build:web` の後に Rust バイナリを再ビルドしてください。Cargo は TypeScript と生成物の鮮度を検証しません。HTML/CSS の変更にも Rust の再ビルドが必要です。実行時の Node.js・npm、外部 CDN は不要です。SPACE の描画には WebGL2 が必要です。

| CLI オプション | 動作 |
|---|---|
| `--listen ADDRESS` | 待受アドレス。既定は `127.0.0.1:8080` |
| `--allow-non-loopback` | 非 loopback での待受を明示的に許可。認証・TLS なしでプロセスメモリや環境変数を公開するため注意 |
| `--help` / `--version` | ヘルプ / バージョン表示 |

SIGINT（Ctrl+C）または SIGTERM で収集停止と HTTP サーバーの終了処理を行います。起動・サーバーの致命的な失敗は非ゼロ終了です。

### crates.io 公開前の検証

公開パッケージには `Cargo.toml` の `include` で生成済みの `dist/web/*.js` を含めます。Git では引き続き生成物を管理しません。公開前に次の手順で生成物を更新し、パッケージ単体でビルドできることを検証します。

```sh
npm ci
npm run build:web
cargo publish --dry-run
```

`cargo publish --dry-run` は実際には公開しません。`cargo install procinsh --locked` では同梱済みの JavaScript を使うため、インストール先に Node.js・npm は不要です。Rust とネイティブ・BPF のビルド依存は必要です。

## 実装構成

| 場所 | 役割 |
|---|---|
| `src/main.rs` | CLI、ロガー初期化、起動設定の検証 |
| `src/http_server/` | サーバー起動・終了、router・middleware、worker の停止・join |
| `src/http_server/api/` | HTTP 入力・応答、domain error のステータス変換、JSON と SSE (Server-Sent Events) |
| `src/http_server/web.rs` | 静的 Web UI 配信 |
| `src/http_server/process/monitoring/` | 接続ごとの独立した観測、60秒の履歴、最新状態の配信 |
| `src/http_server/process/` | `/proc` の解析、PID 識別、プロセス・スレッド・メモリ・FD・ソケット・シグナル情報 |
| `src/http_server/process/snapshot/` | 初回・10秒ごとの ptrace と継続 perf によるレジスタ・スタック取得、framehop による DWARF CFI unwind、逆アセンブル |
| `src/http_server/process/snapshot/symbol/` | ELF 取得・キャッシュ、アドレス変換、ELF/DWARF によるシンボル・ソース位置の解決 |
| `src/http_server/process/snapshot/stack.rs` | サンプルとシンボル解決で共有するフレーム型 |
| `src/http_server/system/` | 全プロセスの構造、subscription、BPF 収集、名前解決 |
| `src/web/` | TypeScript の通常画面・SPACE 画面・描画モデル、HTML/CSS、同梱 Three.js |
| `tests/` | Rust・ブラウザ・ログ・実機センサーのテストと C fixture |

`main.rs` が唯一の crate root で、CLI の検証後は `http_server::run` だけを呼びます。Rust library API は提供しません。モジュールは直接の利用者の最小共通祖先に置き、子モジュールの宣言は private、親に必要な item は原則 `pub(super)` とします。

階層モジュールは `foo.rs` + `foo/` で表し、`mod.rs` は使用しません。`foo.rs` は module documentation、子モジュール宣言、re-export のみを持ち、型・関数・定数の実装は責務を表す子ファイルに置きます。

HTTP の起動と終了は `http_server/server.rs`、共有状態は `state.rs`、router の組み立ては `router.rs` と `api/router.rs`、HTTP guard とアクセスログは `middleware.rs` が担当します。process façade の実装は `process/identity.rs` と `process/resources.rs`、観測の lifecycle は `monitoring/service.rs`、snapshot の orchestration は `snapshot/capture.rs` に置きます。symbol は `symbol/cache.rs` と `symbol/resolve.rs`、system monitoring は `system/service.rs` と状態ログの `status.rs` に分けています。 `activity.rs` は BPF センサーの所有・収集・状態と欠落数の管理を担当します。依存関係図はこれらの子モジュールも含めて生成され、概要図では従来どおり各サブシステムへ集約されます。

HTTP handler と system monitoring は `process.rs` の façade だけを利用します。`process` 内部の `monitoring` は継続観測、`snapshot` は初回の短時間停止と継続的な非停止の詳細取得を担当します。各 collector の sampler が perf fd・mmap と ELF キャッシュを所有します。`process` / `system` は Axum 型に依存しません。

例外として、façade から再公開する domain 型と subscription の操作は `pub(in crate::http_server)` に限定しています。private な子モジュールから親で再公開するために必要な可視性であり、crate 外部への公開ではありません。内部テストは各モジュールに置き、`tests/http.rs` はバイナリを起動して HTTP と SSE、SIGTERM による終了を検証します。

Tokio/Axum が HTTP と SSE を処理し、ブロッキングする詳細 API は `spawn_blocking` に渡します。プロセスの定期観測とシステム全体の構造・活動収集は OS スレッドで動きます。プロセス詳細監視は接続ごとに独立した状態を持ち、`AppState` は一覧探索と各サブシステムを保持します。接続数・collector は process monitoring、perf のライフサイクルとシンボルキャッシュは各 collector が管理します。

Web UI の API 型は `src/web/api-types.ts` に定義し、Rust の JSON 応答と合わせて管理します。null の扱いや16進文字列のアドレスも契約に含まれます。これらはコンパイル時の型で、実行時の入力検証ではありません。TypeScript と Three.js の型定義はビルド専用の npm 依存です。

[API Documentation](https://akawashiro.github.io/procinsh/) に Rust 側のドキュメントがあります。
各 façade の module root にある `//! # Interface` コメントに、再エクスポートする型・関数シグネチャを記載します。
名前のリンクは定義元の rustdoc ページを開き、その **Source** リンクから実装を参照できます。
CI は `rustdoc::broken_intra_doc_links` をエラーにしてリンク切れを検出しますが、コメント内のシグネチャ一致までは検証しません。

## HTTP API

JSON のプロセス識別子は `{ "pid": 123, "start_time_ticks": 456 }` です。アドレスは JavaScript の整数精度を保つため16進文字列で返します。

対象を扱うAPIは毎回識別子を明示します。事前の選択操作やSSE接続は不要です。以下の「識別子クエリ」は必須の `pid` と `start_time_ticks` を意味します。

| Method / path | 引数 | 返り値 |
|---|---|---|
| `GET /api/config` | なし | バージョン、更新間隔 `interval_ms`、履歴秒数 `history_seconds` |
| `GET /api/processes` | なし | プロセス識別子・名前・ユーザー・CPU/RSSなどの一覧 |
| `GET /api/processes/observation` | 識別子クエリ | 要求時のプロセス観測。CPU使用率と毎秒増分は null |
| `GET /api/processes/threads` | 識別子クエリ | 要求時のスレッド観測の配列。CPU使用率は null |
| `GET /api/processes/maps` | 識別子クエリ | `process_id`、maps/smaps、rollup、取得時刻、取得エラー |
| `GET /api/processes/fds` | 識別子クエリ | FD、接続候補、共有所有者、探索警告 |
| `GET /api/processes/environment` | 識別子クエリ | 環境変数の名前・値の一覧と取得情報 |
| `GET /api/processes/auxv` | 識別子クエリ | 補助ベクトルのタグ・値・参照先の解決結果 |
| `GET /api/processes/signals` | 識別子クエリ | プロセス・スレッドのシグナル状態と警告 |
| `GET /api/processes/events` | 識別子クエリ | SSE `observation`：指定プロセスの概要・最新観測・スレッド・履歴・マップ・終了状態 |
| `GET /api/system/events` | なし | SSE `snapshot`・`activity`・`gap`：構造、CPU・IPC・ファイルI/O活動、配信欠落 |

識別子クエリの欠落・構文不正は400、JSON本文の必須フィールド欠落や型不正は422です。PIDは正の整数である必要があります。対象の終了・PID再利用は410で返します。不正なアドレス・範囲は400、その他の観測処理の失敗は原則422、ブロッキングタスクの失敗は500です。observation/threads/mapsを含め、終了済みプロセスの単発GETは成功しません。

SSE は `Content-Type: text/event-stream` で接続を維持し、`event:` にイベント名、`data:` に JSON を送ります。接続直後に送るのは `GET /api/processes/events` が `observation`、`GET /api/system/events` が `snapshot` です。keep-alive はデータの更新ではありません。

### `GET /api/processes/events`

`GET /api/processes/events` は SSE で `observation` イベントを送ります。`?pid=123&start_time_ticks=456` のように識別子クエリを指定します。初回観測を取得して接続を開始し、その後は定期観測時（既定1秒）に次の対象状態全体を送ります。差分ではなく、履歴と保持済みマップも毎回含みます。イベント全体が null になることはありません。

各接続は独立して収集し、履歴は接続時から直近60秒分を保持します。同じプロセスを複数タブで開いた場合も観測・履歴を共有しません。切断後の再接続は新しい履歴で始まります。詳細SSEは最大32接続で、上限超過は429、終了処理中の新規接続は503です。

| フィールド | 配信内容 |
|---|---|
| `summary` | `identity`（PID・開始時刻）、名前、実行ファイル、コマンドライン、実・実効ユーザー、状態、CPU使用率、RSS、スレッド数の概要 |
| `observation` | 最新の `timestamp`、`process_id`、`cpu_percent`、`rss_bytes`、`vms_bytes`、minor/major fault、context switch累計、`io` のread/writeバイト累計、`rates` の毎秒増分、`threads`、CPU番号・nice・priority。未取得なら null |
| `observation.threads` | TID、名前、状態、CPU番号・使用率、priority・nice、scheduler、affinity、context switch累計 |
| `live_samples`、`sampling_error` | TID ごとの最新レジスタ・スタック・命令・sample age・欠落数とサンプラーのエラー |
| `history` | 直近60秒の `timestamp`・`cpu_percent`・`rss_bytes`・`vms_bytes` の配列 |
| `maps` | 仮想メモリ領域の開始・終了、権限、ファイルオフセット、device/inode、パス、RSS/PSS |
| `maps_captured_at`、`maps_error` | マップの取得時刻と取得エラー。未取得時の時刻、エラーなしの場合のエラー値は null |
| `rollup` | RSS・PSS・private bytesの集計。取得できなければ null |
| `exited`、`error` | 対象の終了フラグと観測エラー。エラーなしなら null |

時刻はUnix epochからのミリ秒、メモリ量はバイト、CPU使用率は1コアを100%とします。`rates` はfault・context switchが回/秒、read/writeがバイト/秒です。差分がない初回のCPU使用率や算出不能なrate、取得不能な任意項目は null になり、ゼロとは区別します。`summary` は接続開始時の概要で、継続的に更新される値は `observation` を参照します。

マップは約5秒間隔で取得するため、イベントの最新観測時刻とマップの取得時刻は一致しません。終了を検出したら `exited: true` を含む最終状態を配信し、ストリームを終了します。レジスタ・コールスタック・逆アセンブルは live_samples で配信します。FD詳細・環境変数・auxv・シグナル詳細は個別APIで取得します。

### `GET /api/system/events`

`GET /api/system/events` は SSE で以下のイベントを送ります。時刻 `captured_at` はUnix epochからのミリ秒、`window_ms` は集計期間のミリ秒です。

この API は接続そのものを閲覧セッションとして扱います。同時接続は最大32本で、上限超過は429、アプリ終了後の新規接続は503です。

| イベント | 配信内容とタイミング |
|---|---|
| `snapshot` | 接続直後の保持済み構造、約1秒の待機を挟む構造更新、配信欠落後の再同期。全体を置き換えるデータ |
| `activity` | 最大10Hzの活動集計。`captured_at`、`window_ms`、`cpu`、`ipc`、`files`、`status` |
| `gap` | 購読遅延時の `{dropped_frames: 件数}`。続けて最新 `snapshot` を送り、失われた活動は再送しない |

`system/snapshot.rs` の `SystemSnapshot` を配信します。`Process` はプロセス、`FdEndpoint` はプロセスの FD 端点、`FdRelation` は socket・pipe・共有所有の関係を表します。

`snapshot` のトップレベルは `captured_at`、`processes`、`fd_relations`、`warnings`、`inspected_processes`、`inspected_fds` です。初回収集前は空の構造の場合があります。

- `processes`：`identity`、`parent_id`、名前、実・実効ユーザー、`maps`、`maps_epoch`、`maps_error`。親を特定できなければ `parent_id` は null、マップ取得失敗時はエラーを含みます。
- `fd_relations`：接続ID、端点 `endpoint`・`peer`、label、socket情報、`candidate`・`shared`。端点にはプロセス識別子、FD、FD数、resource、kind、accessがあります。`peer` はローカルの相手を持たなければ null です。`candidate` は接続候補、`shared` は同じリソースの共有で、一意な通信相手とは区別します。
- `socket`：protocol、state、local/remoteアドレス、network_peer、remote_hostname。socket情報や未取得のアドレス・名前は null になり得ます。
- `warnings` と探索件数：取得不能・打ち切りなどの警告と、走査したプロセス・FDの件数。

`activity` の各配列はその集計窓の情報で、履歴全体ではありません。

| フィールド | 要素の内容 |
|---|---|
| `cpu` | `process_id`、`runtime_ns`（実行時間、ナノ秒）、`switches`（切替回数）、`running_threads`（実行中スレッド数）、`cpus`（実行中CPU番号） |
| `ipc` | `process_id`、`resource`、`write`、`bytes`、`count`。送信／書き込みがtrue、受信／読み取りがfalse。ペイロードは含まない |
| `files` | `process_id`, `file: {device: {major, minor}, inode, generation}`, `path`, `write`, `bytes`, `count`。inode は十進文字列。パスが取得不能なら null。 |
| `status` | `active`、CPU・IPC・filesのセンサー状態、観測範囲の説明、`lost`・`files_lost`・`unresolved` などの収集統計 |

識別情報は構造化されています。IPC 活動と system snapshot の FD の `resource` は `{kind: "pipe" | "socket", device: {major, minor}, inode: "…"}`、memory map の `device` は `{major, minor}` です。inode は全て十進文字列で送ります。

ソケットの protocol は `{kind: "tcp" | "udp", family: "ipv4" | "ipv6"}` または `{kind: "unix", socket_type: {kind, code?}}`、state は `{kind, code?}` です。未知のコードは数値を保持します。INET の local/remote は `{ip, port}`、UNIX パスは `path` に分離しています。FD の access は `read`, `write`, `read_write`, `unknown`、kind は `pipe`, `socket`, `fifo` です。

thread の scheduler は `{kind, code?}`、affinity は両端を含む `{start, end}` の配列（取得不能は null）です。シグナルの queued は `{count, limit}`（十進文字列）、signals は `{number, name}` の配列です。mask の hex は精度を保持する16進文字列です。

register の mapping は `{pathname, readable, writable, executable, private}` または null、offset は16進文字列または null、kind は分類 enum の snake_case 名です。memory map の permissions 文字列は廃止し、権限 boolean から表示を生成します。

センサー状態は `{state: "idle" | "starting" | "observing"}` または `{state: "unavailable" | "error", message: "…"}` です。ログ文面は従来どおりですが、レベルの判定は enum に基づきます。

該当活動がない場合やセンサーが利用不能の場合、活動配列は空になります。空配列だけで「活動がなかった」とは判断せず、`status` の各センサーの `state`（observing・unavailable・error） なども確認します。

## バックエンド側の処理

### 共通処理と状態管理

Axum がルートごとにクエリや JSON を取り出し、ハンドラへ渡します。Host・Origin・Fetch Metadata の検証を通過した応答には no-store とセキュリティヘッダを付けます。ブロッキングするプロセス観測 API は `spawn_blocking` で実行し、`AppState` 内の状態は mutex で保護します。

対象は PID 単独ではなく `{pid, start_time_ticks}` で識別し、要求ごとに実際のプロセスの識別子と生存を確認します。サーバー全体の選択対象はありません。別のプロセスや別タブからの要求に依存せず、SSEなしでも単発APIを利用できます。

### 設定とプロセス一覧

- `GET /api/config`：パッケージのバージョン、起動時に指定した更新間隔、履歴の保持秒数を返します。
- `GET /api/processes`：要求ごとに `Discovery` が `/proc` を走査し、プロセス識別子、名前、コマンド、ユーザー、メモリ量などを返します。前回の収集値との差分から CPU 使用率を求めます。初回など差分がない場合は null です。

### 要求時の観測・スレッド・マップ

次のAPIは識別子を検証し、要求ごとに `/proc` を読み取ります。SSE接続の保持状態は参照せず、ptraceによる停止も行いません。

- `GET /api/processes/observation`：CPU・RSS/VMS・fault・I/O・context switch・スレッドなどの単発観測を返します。比較対象となる前回観測を持たないため、CPU使用率と `rates` は null です。
- `GET /api/processes/threads`：単発観測の threads を返します。スレッドごとのCPU使用率は null です。
- `GET /api/processes/maps`：maps/smapsとrollupを読み、取得時刻・取得エラー・process_idとともに返します。取得前後に識別子を確認します。

継続的なCPU使用率と毎秒増分、履歴はSSEで取得します。通常の `/proc` 読み取りは対象を停止せず、各項目の取得時点は厳密には一致しません。

### FD と接続先

`GET /api/processes/fds` は識別子・生存を確認し、pipe/FIFO/socket の FD、アクセス方向、接続候補、同じリソースの共有所有者を収集します。UNIX domain socket の通信相手は socket diagnostic を使って調べます。TCP/UDP ソケットの通信相手の候補は、対象プロセスが属するネットワーク名前空間の情報から探索します。共有所有者と通信相手は区別し、データを消費する読み取りは行いません。探索は3秒・100,000 FD・一致8192 FDを上限とし、打ち切りなどを結果に含めます。

実装は [FD 情報の収集](../src/http_server/process/fds.rs)、[通信相手の候補の照合](../src/http_server/process/fds.rs)、[候補を所有するプロセス・FD の探索](../src/http_server/process/fds.rs) を参照してください。UNIX domain socket の通信相手の inode を取得する処理は [socket diagnostic](../src/http_server/process/sockets.rs) にあります。

### 環境変数・補助ベクトル・シグナル

いずれも識別子・生存を確認して要求時に取得します。定期観測に含めて再収集するものではありません。

これらの API は procfs に公開されたファイルを読み取ります。auxv の文字列参照先だけは、追加で `process_vm_readv` を使って対象プロセスのメモリから取得します。

- `GET /api/processes/environment`：procfs の [`/proc/<pid>/environ`](https://man7.org/linux/man-pages/man5/proc_pid_environ.5.html) を最大1 MiB読み取り、NUL 区切りの各項目を最初の `=` で名前と値に分けます。重複名・空値・値中の `=` を維持します。通常は exec 時の環境領域であり、起動後の変更すべてを反映しません。実装は [ファイルの読み取り](../src/http_server/process/details.rs) と [環境変数の解析](../src/http_server/process/details.rs) を参照してください。
- `GET /api/processes/auxv`：`/proc/<pid>/exe` の ELF ヘッダから32/64 bitを判別し、procfs の [`/proc/<pid>/auxv`](https://man7.org/linux/man-pages/man5/proc_pid_auxv.5.html) を最大64 KiB読み取ります。タグと値の組として解析し、既知・未知のタグを扱います。`AT_EXECFN`・`AT_PLATFORM`・`AT_BASE_PLATFORM` の文字列参照は、`process_vm_readv` で最大4096バイトまで解決します。参照先が読めなくても数値は保持します。big-endian ELF は対象外です。実装は [ELF ヘッダと auxv の読み取り](../src/http_server/process/details.rs)、[文字列参照の解決](../src/http_server/process/details.rs)、[メモリの読み取り](../src/http_server/process/memory.rs) を参照してください。
- `GET /api/processes/signals`：procfs の [`/proc/<pid>/status`](https://man7.org/linux/man-pages/man5/proc_pid_status.5.html) と、各スレッドの [`/proc/<pid>/task/<tid>/status`](https://man7.org/linux/man-pages/man5/proc_pid_task.5.html) を読み取ります。`SigPnd`（スレッドの保留）・`ShdPnd`（プロセス全体の保留）・`SigBlk`（ブロック）・`SigIgn`（無視）・`SigCgt`（ハンドラ登録）の16進マスクを解析します。最大4096スレッド・2秒で打ち切ります。受信履歴や送信元の追跡、シグナル送信は行いません。実装は [status の読み取りとスレッドの列挙](../src/http_server/process/signals.rs) と [シグナル状態の解析](../src/http_server/process/signals.rs) を参照してください。

### 初回・定期スナップショットと非停止ライブサンプリング

各 SSE collector は初回 watch 値の配信前にスレッドを列挙し、通常の perf event を開いてから、各 TID を独立に `PTRACE_SEIZE` → `PTRACE_INTERRUPT` → `waitpid(__WALL)` で停止します。`PTRACE_GETREGSET(NT_PRSTATUS)` と `process_vm_readv` で同じ停止状態のレジスタと RSP から最大16 KiB（含有 mapping の末尾まで）のスタックを取得し、成功・失敗ともに detach します。初回・10秒ごとの ptrace と継続 perf は共通の `RawSample` と unwind・シンボル解決・レジスタ分類を使います。ptrace の CPU は null です。ptrace は初回に加え、取得完了から10秒後に全 TID を再取得します。perf で更新中の TID も対象です。遅延時の連続取得はせず、失敗しても次回試行は10秒後です。取得できない TID があっても前回サンプルを保持し、perf 監視を継続します。定期取得結果は通常の観測周期で配信します。

各 SSE collector は TID ごとに `perf_event_open(pid=tid, cpu=-1)` で software CPU-clock event を開き、CPU migration に追従します。初期周期は実行中の user CPU 時間10ms（約100Hz）、user stack dump は16384バイトです。待機遷移には追加の `PERF_COUNT_SW_CONTEXT_SWITCHES` event（周期1、stack dump 16384バイト）を使います。`context_switch` と `sample_id_all` を有効にし、同じ ring の次の SWITCH_OUT record の TID・CPU・時刻・PREEMPT flag を確認して voluntary / preempted を区別します。対応する sample がない場合や loss / throttle / switch-in の際は対応付けを破棄します。両 event の最新サンプルを monotonic 時刻で比較し、新しいものだけを採用します。TID/TIME/CPU/REGS_USER/STACK_USER を ring buffer から読み、最新値だけを保持します。約50msごとに drain と thread 追加・終了確認を行います。TID と開始時刻を確認して再利用を検出し、プロセス識別子も採取前後に検証します。

レジスタ18個とスタックは同じ perf sample または ptrace 停止状態に由来します。framehop は sampled RSP/RBP/RIP と採取済みバイトだけで最大256 frame を unwind します。Sampler ごとに module と rule cache を保持し、実行可能 mapping の変更時に再構築します。ELF の取得は既存の device/inode 検証とキャッシュを使い、PIE・非 PIE・共有ライブラリの load bias を補正します。`.eh_frame` / `.eh_frame_hdr`（header がなければ CFI index）と `.debug_frame` を読み、metadata が利用できない場合は framehop の frame-pointer fallback を使います。`.debug_frame` がある ELF はこれを優先し、CRT だけの `.eh_frame` に application の CFI が隠れることを防ぎます。シンボル・ソース・inline-frame 解決は既存の処理を再利用します。

`unwind_stop` は正常終了、256-frame 上限、stack snapshot の取得範囲不足、範囲外アドレス、module 取得失敗、module の metadata 不足を示します。後二者は fallback を使用した旨を添えます。section が存在しても個別の PC の CFI が欠ける／対応外の場合、framehop は内部で fallback するため、その詳細までは区別できません。unwind 中に live stack を追加取得したり ptrace stop したりしません。実行可能 mapping が変わるまで取得に失敗した module は再取得しません。

SSE の `live_samples` は TID、`sampled_at`（Unix ms）、`sample_age_ms`（monotonic clock）、CPU、`lost_samples`、registers、call_stack、disassembly、unwind_stop、error を含みます。未採取は時刻 null と Waiting for sample、採取済みの値は取得時刻と経過時間を表示します。sleeping/blocked thread も10秒ごとの ptrace で更新を試み、それ以外は最後の値を保持します。保存された user-space の状態を表示し、眠っている thread の現在値ではありません。観測開始前から眠っている thread も ptrace が許可されれば初回配信で採取できます。初回取得に失敗した場合は次の実行／switch-out または10秒後の ptrace 再試行を待ちます。取得時刻、経過時間と制約を画面に表示します。取得元や voluntary / preempted の区別は内部で保持し、GUI・HTTP API には公開しません。全スレッドの同時点状態は保証しません。perf 権限不足は thread error、collector エラーは `sampling_error` で表示し、通常観測は継続します。

逆アセンブルは sampled RIP から `process_vm_readv` で後読みする best-effort 表示です。最大256バイト、最大32命令を iced-x86 で decode します。JIT/self-modifying code の命令バイトと sample 時点の RIP は整合しない場合があります。32-bit compatibility mode は対象外です。

PERF_RECORD_LOST と ring overrun/不正レコードを欠落として保持します。thread 終了、対象終了、SSE 切断、サーバー終了で RAII により fd/mmap を解放します。CAP_SYS_PTRACE と process_vm_readv は ptrace スナップショット・auxv・live disassembly に使います。

### プロセス詳細監視の SSE 配信処理

`GET /api/processes/events` は接続枠を確保して識別子を検証し、初回観測・マップを取得します。接続専用のOSスレッドとwatch channelを作り、設定間隔で観測して直近60秒の履歴を更新します。maps/smapsは約5秒ごとに更新します。

レスポンスのストリームがRAIIガードを所有し、未読のまま破棄された場合も接続枠を解放して停止フラグを設定します。収集中の処理は完了後に停止します。アプリ終了時にも停止し、終了処理は収集スレッドをjoinします。対象終了時は最終状態を送り、収集とストリームを終了します。

watch channelは接続ごとに独立し、遅い購読者へ古い状態を蓄積せず最新値を送ります。keep-aliveは10秒間隔です。ネットワーク断の検出が遅れれば、検出まで収集が残ることがあります。

### システム全体の状態・構造と閲覧セッション管理

- `GET /api/system/events`：接続数を上限確認と同時に加算し、初回に収集ワーカーを起動します。レスポンスのストリームがRAIIガードを所有し、未読のレスポンスも含めて終了・破棄時に接続数を減らします。
最後の接続がなくなると、ワーカーが次に状態を確認した時点で収集を休止してセンサーを解放します。ワーカースレッドはアプリ終了まで残り、再接続で収集を再開します。ネットワーク断ではサーバーの切断検出が遅れる場合があり、収集停止までの時間に上限は設けていません。

構造収集・配信の完了後、約1秒待って次の収集を開始します。実際の更新周期は処理時間と待機時間の合計です。全体 FD 走査は100,000 FD・4秒、各 PID のマッピングは4096件、接続図は約20,000接続を上限とします。プロセスの親子・マップ・pipe/socket・ネットワーク接続先を非停止で探索し、取得不能・打ち切り・欠落を状態として扱います。ネットワーク接続先の名前解決結果はキャッシュします。

### システム全体の活動収集と SSE 配信処理

`GET /api/system/events` は閲覧者を登録して broadcast channel を購読します。最初に保持済みの構造を `snapshot` として返し、その後は構造・活動を配信します。センサー状態と収集統計は `activity` イベントの `status` に含まれます。切断・配信終了で登録を解除し、アプリ終了時にはストリームを終了します。

購読側が遅延した場合は `gap` と最新の `snapshot` を送り、失われた活動を再生しません。keep-alive は10秒間隔です。活動は最大10Hzで集計・配信します。

いずれのセンサーも CO-RE eBPF で実装しています。CPU scheduling・IPC・ファイル I/O は独立した eBPF プログラムで収集し、各センサーのロード・状態・解放も独立しています。Space では eBPF の `CpuActivity` を約100ms周期の scheduling activity・CPU glowに使います。scheduler event はカーネルの map で集約し、userspace へ逐次転送しません。

| センサー | バックエンドの観測内容と制約 | eBPF ソース |
|---|---|---|
| CPU | `sched_switch` で実行時間と実行中 CPU を集計 | [sched.bpf.c](../src/http_server/system/sched.bpf.c) |
| IPC | pipe read/write と socket の送受信結果を観測。ペイロードは読まず、MSG_PEEK は加算しない。splice/sendfile、一部 io_uring、帰属不明のワーカーは対象外 | [ipc.bpf.c](../src/http_server/system/ipc.bpf.c) |
| ファイル I/O | VFS の read/write、ベクトル I/O の成功バイト数と回数を観測。ページキャッシュ経由も含む。mmap、io_uring、splice/sendfile、物理ディスク転送量は対象外 | [files.bpf.c](../src/http_server/system/files.bpf.c) |

ファイルのパスは操作時に取得し、取得できない場合は device/inode 等の識別子を使います。BPF のフックが利用できない場合はセンサーごとの理由を状態 API とログに出し、利用可能な情報の収集を継続します。必要なカーネル機能・権限はセンサーごとに異なります。

## フロントエンド側の処理

### プロセス一覧 `/list`（`/` も同じ一覧を表示）

一覧と詳細は同じ HTML と `app.ts` を使います。`/` は常に一覧を表示し、詳細SSEを開きません。対象の選択はそのタブ内だけで管理します。

| 利用API | 呼び出すタイミングと用途 |
|---|---|
| `GET /api/config` | 初期化時に更新間隔を取得 |
| `GET /api/processes` | 一覧表示時と定期更新時に一覧を取得 |

一覧の更新間隔は設定値と1000msの大きい方で、詳細表示中と取得中には一覧の更新を行いません。検索と CPU・RSS・PID による並べ替えは取得済みデータに対してブラウザ内で行います。行を選ぶと取得済みの識別子を指定して詳細SSEを開き、最初の観測で詳細表示へ切り替えます。`history.replaceState` で URL を更新します。一覧のタイトルは `procinsh / list`、一覧と詳細のヘッダーにある Go to space view は `/space` への下線付きリンクです。Go to list view は `/list` に移動します。

### プロセス詳細 `/process/{pid}`

| 利用API | 呼び出すタイミングと用途 |
|---|---|
| `GET /api/config` | 一覧と共通の初期化 |
| `GET /api/processes` | 直接URLアクセス時に一覧からPIDの開始時刻を解決 |
| `GET /api/processes/events` | 識別子クエリを付けて接続し、概要・観測・履歴・スレッド・マップ・終了状態を更新 |
| `GET /api/processes/environment`、`GET /api/processes/auxv`、`GET /api/processes/fds`、`GET /api/processes/signals` | 各パネルを初めて開くときと再取得操作時 |

プロセス詳細を開くとライブサンプルを表示します。スレッドを選択すると最新のレジスタ・スタック・命令を表示し、sample age と欠落数を示します。Call Stack は詳細グリッドの全列にまたがって表示します。

直接アクセス時は一覧からPIDの開始時刻を解決し、その識別子でSSEを接続します。PIDが一覧にない場合は一覧と終了エラーを表示します。全ての追加GETにも識別子クエリを付けます。通常の更新には `observation` を使い、observation・threads・mapsの単発GETは直接呼びません。

対象切替やGo to list viewでは現在のSSEを閉じ、保持した詳細情報をリセットします。他タブには影響しません。接続世代と識別子を照合して古い通知を無視します。通信切断ではEventSourceが同じ識別子で再接続し、履歴は再開始します。同じPIDの別プロセスへは自動で乗り換えません。`exited: true` を受信したら接続を閉じ、最終状態と終了表示を残します。ページ離脱時は閉じ、ブラウザのページキャッシュから復帰した場合は同じ識別子で接続し直します。タイトルは `procinsh / <process name>` です。

受信した履歴から CPU/RSS のグラフを描画し、スレッドやマップを表示します。各スレッドの最新サンプルを保持し、選択したスレッドのレジスタ・スタック・逆アセンブルを表示します。マップ・レジスタ・スタック・逆アセンブル・auxvのアドレスはテキストとして表示します。環境変数などの文字列は HTML として解釈せず表示し、検索は取得済みデータを使います。

対象変更時には保持した詳細情報をクリアします。追加パネルでは、識別子と要求世代を確認して古い応答を破棄します。追加パネルの再取得が失敗した場合は、以前の結果があれば残したうえでエラーを表示します。

### グラフ `/space`

| 利用API | 呼び出すタイミングと用途 |
|---|---|
| `GET /api/system/events` | 表示開始・再表示時に接続し、構造・活動を受信。接続中だけ閲覧者として登録 |

初期構造も SSE から取得します。通常画面の `/api/processes` 配下の API も呼びません。グラフ上の選択はブラウザ内だけで管理し、詳細へのリンクは `/process/{pid}` に移動します。

Three.js でプロセスの親子関係、仮想アドレス空間、接続先、ファイル I/O を3D表示します。マップのアドレスの隙間を圧縮し、高さを正規化するため、プロセス間の同じ高さは同じアドレスを意味しません。検索、選択、カメラ操作、再配置もブラウザ内の処理です。タイトルは `procinsh / graph` です。

- `snapshot`：構造を更新し、プロセスと接続先の配置を維持しながら追加・削除を反映します。
- `activity`：CPU の発光、IPC・ネットワークの流れ、ファイル I/O の表示を更新します。CPU の発光は実行中に強まり、活動が途絶えると約500msで減衰します。ファイル表示は最終アクセスから30秒、各プロセス32個・全体512個まで保持します。
- `gap`：描画中の粒子をクリアし、続く構造イベントを反映します。

タブ非表示・ページ離脱時は SSE と再接続タイマーを止め、活動表示をクリアします。再表示時は接続し直します。接続エラーでは現在の EventSource を閉じ、エラーを表示して、表示中に限り3秒後に新しい接続を作ります。自動再接続との二重実行を避け、古い接続からのイベントは無視します。接続成功時にエラー表示を消します。

## 権限とログ

`process_vm_readv` は所有者、dumpable 属性、Yama、`CAP_SYS_PTRACE`、seccomp などの制約を受けます。BPF はカーネル側の対応と観測権限も必要です。権限やカーネル設定の自動変更、sudo の自動実行はしません。

待受の既定値は `127.0.0.1:8080` です。IPv4/IPv6 の loopback（`127.0.0.1`、`::1` など）は追加フラグなしで利用できます。LAN アドレスや wildcard（`0.0.0.0`、`::`）など非 loopback の指定は、`--allow-non-loopback` がなければ bind 前に非ゼロ終了します。明示的に許可する例は `procinsh --listen 0.0.0.0:9090 --allow-non-loopback` です。許可した場合も警告を出します。

認証・TLS はありません。接続できる利用者はプロセスメモリや環境変数にアクセスできるため、非 loopback での待受はアクセス範囲を管理した信頼できるネットワーク内に限定してください。Host/Origin/Fetch Metadata の検証と API レスポンスの `Cache-Control: no-store` は維持しますが、これらは認証の代わりにはなりません。

ログは `log` と `env_logger` を使い、標準エラーに時刻・レベル・出力元のファイルパスと行番号（例：`src/http_server/system/service.rs:123`）を出します。既定は `info` です。`RUST_LOG` の絞り込みには引き続きモジュール名を使います。

- `info`：起動・終了、観測の開始・停止、対象の終了、収集状態と復旧。
- `warn`：観測失敗、センサー利用不可。同じ状態・エラーの連続出力を抑制。
- `error`：致命的な実行失敗、ワーカー異常、HTTP 500系。
- `debug`：HTTP のメソッド・パス・ステータス・応答生成時間、API エラー詳細、構造収集件数、SSE のイベント送出。SSE は接続ごとに API パスとイベント名を記録し、`/api/system/events` では `payload_bytes`（UTF-8 の JSON 本文のバイト数。SSE の行形式・HTTP ヘッダーは含まない）も記録します。プロセス観測では識別子、配信欠落では欠落件数も記録します。ペイロードと keep-alive は記録しません。送出ログはサーバーがストリームにイベントを渡したことを示し、クライアントの受信完了を保証しません。

```sh
sudo env RUST_LOG=procinsh=debug ./target/debug/procinsh --listen 127.0.0.1:9090
sudo env RUST_LOG=info,procinsh::http_server::system=debug ./target/debug/procinsh --listen 127.0.0.1:9090
```

HTTP アクセスログだけを絞り込む例は `RUST_LOG=info,procinsh::http_server::middleware=debug` です。既存のサブシステム単位のフィルタは子モジュールにも適用されます。

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
# ローカルの権限付き単体テスト（perf のスキップを禁止）
PROCINSH_REQUIRE_PERF=1 ./scripts/dev_test.sh
PROCINSH_BINARY=./scripts/dev_run.sh python3 tests/listen-policy.py
node tests/space-model.mjs

# フォーマット・静的解析
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
RUSTDOCFLAGS="-D rustdoc::broken_intra_doc_links" cargo doc --locked --workspace --no-deps --document-private-items
```

C ソース・ヘッダー（BPF とテスト用プログラムを含む）は `.clang-format` に従って整形します。clang-format は `requirements-format.txt` の **21.1.8** に固定し、ローカルと CI で同じ `scripts/format_c.py` を使います。スクリプトは実行バイナリのバージョンも検査し、不一致なら失敗します。

```sh
python3 -m venv .venv-format
.venv-format/bin/python -m pip install -r requirements-format.txt
CLANG_FORMAT="$PWD/.venv-format/bin/clang-format" python3 scripts/format_c.py
CLANG_FORMAT="$PWD/.venv-format/bin/clang-format" python3 scripts/format_c.py --check
```

`--check` はファイルを書き換えず、整形差分があれば失敗します。Git 管理対象と未追跡の `.c` / `.h` を検査し、無視された生成物（`vmlinux.h` など）は対象にしません。C の変更時は整形してからコミットしてください。clang-format の更新は固定バージョンの変更と全 C ファイルの再整形を合わせて行います。

CI は Rust と C のフォーマット確認、TypeScript の型チェックとビルド、Rust の全ターゲットのビルド、Clippy、rustdoc のリンク検証、Rust テスト、ログ検証、SPACE モデル検証を実行します。ブラウザと実機センサーのテストは別途実行します。

`Publish rustdoc` workflow は `main` への push と手動実行時だけ動き、rustdoc とモジュール依存関係グラフを生成して GitHub Pages に公開します。PR では通常の `CI` workflow で rustdoc のリンクを検証し、`cargo-modules` のインストールやグラフ生成は行いません。

`cargo build --locked --all-targets` と `cargo test --locked` は Ubuntu 24.04・Ubuntu 26.04・Fedora 44 のコンテナで実行します。matrix は `fail-fast: false` とし、各ディストリビューションの結果を個別に表示します。各コンテナで Node.js 22 と npm をインストールし、`npm ci`、`npm run build:web`（TypeScript 型チェックを含む）から実行します。フォーマット、Clippy、listen policy、SPACE モデルと明示的な bpftool・カーネル BTF 検査は単一環境に残します。各コンテナは Ubuntu 24.04 runner のカーネルと BTF を使うため、この matrix はディストリビューションのユーザー空間の差を検証します。各ディストリビューション固有のカーネルでの BPF センサー動作は検証しません。

matrix の native dependency は Ubuntu では `build-essential clang llvm pkg-config libelf-dev zlib1g-dev python3` に加えて Ubuntu 24.04 は `linux-tools-generic`、Ubuntu 26.04 は `bpftool`、Fedora では `gcc gcc-c++ make clang llvm pkgconf-pkg-config elfutils-libelf-devel zlib-devel python3 bpftool` をインストールします。両方で Rust 導入・checkout に必要な `ca-certificates curl git tar gzip` もインストールします。Ubuntu 26.04 では独立した `bpftool` パッケージの実行ファイルを使います。Ubuntu 24.04 の bpftool は実行カーネルのバージョンに依存する wrapper を避け、`/usr/lib/linux-tools/*/bpftool` の実体を `BPFTOOL` に指定します。

Rust テストは明示的な識別子の必須性、SSEの独立した履歴・切断・接続上限、`/proc` の解析、PID 再利用、メモリ読み取り、perf sample と欠落処理、シンボル、HTTP、システム全体の構造・SSE接続管理・集計を検証します。ログテストは既定レベル、debug、off、標準エラー、SIGTERM、ポート競合、クエリ非出力、IPv4/IPv6 の loopback 起動、非 loopback の bind 前拒否と明示的許可・警告、CLI ヘルプを検証します。プロセス観測を拒否するサンドボックスでは一部テストが失敗するため、テスト対象への perf_event_open/process_vm_readv とローカル通信が許可された環境が必要です。

`./scripts/dev_test.sh [テスト名フィルター] [--nocapture]` は単体テストをビルドし、既存の sudoers で許可された実行パスに一時配置して capability を付けます。テスト成功・失敗のいずれでも元のアプリケーションバイナリを復元します。権限設定と実行は変更していない `dev_run.sh` に委譲します。この実行中は同じ checkout でビルドや dev_run を並行実行しないでください。

perf の実機 fixture は権限が利用できない環境では明示メッセージとともにスキップします。`PROCINSH_REQUIRE_PERF=1` を設定するとスキップを禁止して権限不足も失敗にします。実機テストは `blocked_stack` fixture の100ms nanosleepで3秒間に30回の更新を確認しました（Linux 7.0.0-15-generic）。R12〜R15をasmで既知値にして照合し、12段再帰はmainまで復元、96段再帰（各 frame に256バイトの領域）は16 KiB dumpの範囲で停止することを検証します。全18レジスタの独立照合や全kernelでの精度・性能を保証する検証ではありません。開始前からsleep中のfixtureで初回・定期 ptrace 取得・detach と後続 perf への切り替えを検証します。frame pointer を省略した PIE／非 PIE と `.debug_frame` fixture は実際の call 命令から組み立てた snapshot でも main までの unwind を検証します。実機 fixture には共有ライブラリの再帰から libc の nanosleep を跨いで main まで復元するケースもあります。CAP_PERFMON を持つテストプロセスで busy-loop の更新、再帰 frame、複数 TID、churn の追加・削除、sleep 後の age、対象終了を確認してください。

### ブラウザテスト

上記の Web・Rust ビルドと fixture の準備に加え、Google Chrome または Chromium が必要です。テスト自体は Node.js 標準機能と DevTools Protocol を使い、追加の npm テストライブラリは不要です。

```sh
node tests/browser.mjs
node tests/space-browser.mjs

# ブラウザのパスを指定する場合
CHROME=/usr/bin/chromium node tests/browser.mjs
CHROME=/usr/bin/chromium node tests/space-browser.mjs
```

通常画面は検索・選択・SSE・ライブサンプル・詳細パネル・終了処理を、SPACE は WebGL、配置、選択、ネットワークとファイルの描画を検証します。ブラウザテストは一時サーバーとブラウザプロファイルを作り、終了時に片付けます。画面・モデルの検証と実機センサーの検証は別です。

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

## 関連リンク

- [モジュール依存関係](https://akawashiro.github.io/procinsh/architecture/)
- [Rust doc](https://akawashiro.github.io/procinsh/procinsh/index.html)
