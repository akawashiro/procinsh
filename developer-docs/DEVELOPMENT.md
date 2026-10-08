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

最新 main（9090）と crates.io の公開最新版（9091）を Tailscale 経由で常時見る場合は、[main・公開最新版の常設プレビュー](PREVIEW.md)を参照してください。systemd サービスと、main は約10秒間隔・公開版は約1分間隔の自動更新を利用できます。

HTML/CSS、生成した JavaScript、Three.js（revision 180）はバイナリに埋め込みます。TypeScript を変更したら `npm run build:web` の後に Rust バイナリを再ビルドしてください。Cargo は TypeScript と生成物の鮮度を検証しません。HTML/CSS の変更にも Rust の再ビルドが必要です。実行時の Node.js・npm、外部 CDN は不要です。SPACE の描画には WebGL2 が必要です。

| CLI オプション | 動作 |
|---|---|
| `--listen ADDRESS` | 待受アドレス。既定は `127.0.0.1:8080` |
| `--allow-non-loopback` | 非 loopback での待受を明示的に許可。認証・TLS なしでプロセスメモリや環境変数を公開するため注意 |
| `--help` / `--version` | ヘルプ / バージョン表示 |

SIGINT（Ctrl+C）または SIGTERM で収集停止と HTTP サーバーの終了処理を行います。起動・サーバーの致命的な失敗は非ゼロ終了です。

### crates.io 公開前の検証

公開パッケージには `Cargo.toml` の `include` で生成済みの `dist/web/**/*.js` を含めます。Git では引き続き生成物を管理しません。公開前に次の手順で生成物を更新し、パッケージ単体でビルドできることを検証します。

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
| `src/http_server/process/` | `/proc` の解析、PID 識別、プロセス・スレッド・メモリ・FD・ソケット情報 |
| `src/http_server/process/snapshot/` | 初回・10秒ごとの ptrace と継続 perf によるレジスタ・スタック取得、framehop による DWARF CFI unwind、逆アセンブル |
| `src/http_server/process/snapshot/symbol/` | ELF 取得・キャッシュ、アドレス変換、ELF/DWARF によるシンボル・ソース位置の解決 |
| `src/http_server/process/snapshot/stack.rs` | サンプルとシンボル解決で共有するフレーム型 |
| `src/http_server/system/` | 全プロセスの構造、subscription、BPF 収集、名前解決 |
| `src/web/` | TypeScript の通常画面・SPACE 画面・描画モデル、HTML/CSS、同梱 Three.js |
| `tests/` | Rust・ブラウザ・ログ・実機センサーのテストと C fixture |

`main.rs` が唯一の crate root で、CLI の検証後は `http_server::run` だけを呼びます。Rust library API は提供しません。モジュールは直接の利用者の最小共通祖先に置き、子モジュールの宣言は private、親に必要な item は原則 `pub(super)` とします。

Rust の型名は責務を表します。データ・値は表す内容（`ProcessId`、`ProcessSummary`、`ProcessSnapshot` など）、処理・状態・リソースを管理する型は役割（`ProcessScanner`、`ProcessMonitor`、`FileActivityCollector`、`PerfEventReader`、`PtraceGuard` など）が伝わる名前にします。補助メソッドの有無だけでは分類せず、すべての型に機械的な接尾辞を付けることは求めません。

階層モジュールは `foo.rs` + `foo/` で表し、`mod.rs` は使用しません。`foo.rs` は module documentation、子モジュール宣言、re-export のみを持ち、型・関数・定数の実装は責務を表す子ファイルに置きます。

HTTP の起動と終了は `http_server/server.rs`、共有状態は `state.rs`、router の組み立ては `router.rs` と `api/router.rs`、HTTP guard とアクセスログは `middleware.rs` が担当します。process façade の実装は `process/identity.rs` と `process/resources.rs`、観測の lifecycle は `monitoring/service.rs`、snapshot の orchestration は `snapshot/capture.rs` に置きます。symbol は `symbol/cache.rs` と `symbol/resolve.rs`、system monitoring は `system/service.rs` と状態ログの `status.rs` に分けています。 `activity.rs` は BPF センサーの所有・収集・状態と欠落数の管理を担当します。依存関係図はこれらの子モジュールも含めて生成され、概要図では各サブシステムへ集約されます。

HTTP handler と system monitoring は `process.rs` の façade だけを利用します。`process` 内部の `monitoring` は継続観測、`snapshot` は初回・10秒ごとの ptrace による停止状態の取得と、継続的な perf による非停止の詳細取得を担当します。各 collector の sampler が perf fd・mmap と ELF キャッシュを所有します。`process` / `system` は Axum 型に依存しません。

例外として、façade から再公開する domain 型と subscription の操作は `pub(in crate::http_server)` に限定しています。private な子モジュールから親で再公開するために必要な可視性であり、crate 外部への公開ではありません。内部テストは各モジュールに置き、`tests/http.rs` はバイナリを起動して HTTP と SSE、SIGTERM による終了を検証します。

Tokio/Axum が HTTP と SSE を処理し、ブロッキングする詳細 API は `spawn_blocking` に渡します。プロセスの定期観測とシステム全体の構造・活動収集は OS スレッドで動きます。プロセス詳細監視は接続ごとに独立した状態を持ち、`AppState` は一覧探索と各サブシステムを保持します。接続数・collector は process monitoring、perf のライフサイクルとシンボルキャッシュは各 collector が管理します。

Web UI の API 型は `src/web/shared/api-types.ts` に定義し、Rust の JSON 応答と合わせて管理します。null の扱いや16進文字列のアドレスも契約に含まれます。これらはコンパイル時の型で、実行時の入力検証ではありません。TypeScript と Three.js の型定義はビルド専用の npm 依存です。

[API Documentation](https://akawashiro.github.io/procinsh/) に Rust 側のドキュメントがあります。
各 module root の `//!` は責務と、そのモジュールの外から使う入口を説明します。crate 外部への公開に限らず、親・兄弟モジュール向けの I/F も対象です。
`//! # Interface` には通常ビルドでその root が公開・再公開する型・関数の一覧、型、可視性、関数シグネチャ、定義リンクを記載し、入口の変更時に更新します。シグネチャに登場する補助型は必要な定義リンクで案内します。
子モジュール内部の関数、フィールド、列挙値、テスト専用項目は網羅的に転記しません。公開型のメソッドの詳細、JSON の表現やフォールバックなどの契約、所有権・ロック・処理順序の注意点は定義側に記載し、変更履歴ではなく現在の振る舞いを説明します。
この手動一覧は、固定ツールチェーンの rustdoc が restricted re-export を façade の I/F として表示しない構成を補うためのものです。
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
| `GET /api/processes/events` | 識別子クエリ | SSE `observation`：指定プロセスの概要・最新観測・スレッド・履歴・マップ・終了状態 |
| `GET /api/system/events` | なし | SSE `snapshot`・`activity`・`gap`：構造、CPU・IPC・ファイルI/O活動、配信欠落 |

識別子クエリの欠落・構文不正は400です。PIDは正の整数である必要があります。observation/threads/mapsを含め、単発GETで対象の終了・PID再利用を検出した場合は410で返します。その他の観測処理の失敗は原則422、ブロッキングタスクの失敗は500です。

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

マップは約5秒間隔で取得するため、イベントの最新観測時刻とマップの取得時刻は一致しません。終了を検出したら `exited: true` を含む最終状態を配信し、ストリームを終了します。レジスタ・コールスタック・逆アセンブルは live_samples で配信します。FD詳細・環境変数・auxvは個別APIで取得します。

### `GET /api/system/events`

`GET /api/system/events` は SSE で以下のイベントを送ります。時刻 `captured_at` はUnix epochからのミリ秒、`window_ms` は集計期間のミリ秒です。

この API は接続そのものを閲覧セッションとして扱います。同時接続は最大32本で、上限超過は429、アプリ終了後の新規接続は503です。

| イベント | 配信内容とタイミング |
|---|---|
| `snapshot` | 接続直後の保持済み構造、約1秒の待機を挟む構造更新、配信欠落後の再同期。`kind` に応じて全体または差分を反映 |
| `activity` | 約1秒ごとの活動集計。`captured_at`、`window_ms`、`cpu`、`ipc`、`files`、`status` |
| `gap` | 購読遅延時の `{dropped_frames: 件数}`。続けて最新 `snapshot` を送り、失われた活動は再送しない |

`system/snapshot.rs` の `SystemSnapshot` を、接続ごとの `SnapshotEncoder` で JSON に変換して配信します。`ProcessSnapshot` はプロセス、`FdEndpoint` はプロセスの FD 端点、`FdRelation` は socket・pipe・共有所有の関係を表します。

`snapshot` のトップレベルは `kind`、`sequence`、`captured_at`、`processes`、`warnings`、`inspected_processes`、`inspected_fds` と、FD 関係のデータです。`kind` は `full` または `delta`、`sequence` は接続内で配信ごとに増加する番号です。`delta` は差分の基準となる直前の `sequence` を `base_sequence` に含みます。初回収集前は空の構造の場合があります。

初回、`gap` 後の再同期、前回の `full` から60秒以上経過した構造更新では `full` を送り、全マップと FD 関係を含む構造全体を置き換えます。それ以外は `delta` を送ります。受信側は `base_sequence` を保持した構造の `sequence` と照合して差分を適用します。SPACE の実装は [mergeSnapshot](../src/web/space/model.ts) を参照してください。

- `processes`：毎回、現在の全プロセスの `identity`、`parent_id`、名前、実・実効ユーザー、`maps_epoch`、`maps_error` を含みます。一覧から消えた識別子は削除します。親を特定できなければ `parent_id` は null、マップ取得失敗時はエラーを含みます。
- マップ：`maps` があればそのプロセスのマップを置き換え、`maps_delta` があれば開始アドレスをキーに `upsert` の追加・更新と `remove` の削除を適用します。`delta` で両方を省略した場合は、同じプロセス識別子のマップと `maps_epoch` を保持します。新しい識別子には `maps` を含めます。
- FD 関係：`fd_relations` があれば関係全体を置き換え、`fd_relations_delta` があれば接続IDをキーに `upsert` と `remove` を適用します。マップ・FD 関係とも、差分より全体の JSON が小さい場合は置換形式を使います。
- FD 関係の各要素：接続ID、端点 `endpoint`・`peer`、label、socket情報、`candidate`・`shared`。端点にはプロセス識別子、FD、FD数、resource、kind、accessがあります。`peer` はローカルの相手を持たなければ null です。`candidate` は接続候補、`shared` は同じリソースの共有で、一意な通信相手とは区別します。
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

thread の scheduler は `{kind, code?}`、affinity は両端を含む `{start, end}` の配列（取得不能は null）です。

register の mapping は `{pathname, readable, writable, executable, private}` または null、offset は16進文字列または null、kind は分類 enum の snake_case 名です。memory map の権限は `readable`、`writable`、`executable`、`private` の boolean で表し、画面ではこれらから権限表示を生成します。

センサー状態は `{state: "idle" | "starting" | "observing"}` または `{state: "unavailable" | "error", message: "…"}` です。ログレベルの判定はセンサー状態の enum に基づきます。

該当活動がない場合やセンサーが利用不能の場合、活動配列は空になります。空配列だけで「活動がなかった」とは判断せず、`status` の各センサーの `state`（observing・unavailable・error） なども確認します。

## バックエンド側の処理

### 共通処理と状態管理

Axum がルートごとにクエリを取り出し、ハンドラへ渡します。Host・Origin・Fetch Metadata の検証を通過した応答には no-store とセキュリティヘッダを付けます。ブロッキングするプロセス観測 API は `spawn_blocking` で実行し、`AppState` 内の状態は mutex で保護します。

対象は PID 単独ではなく `{pid, start_time_ticks}` で識別し、要求ごとに実際のプロセスの識別子と生存を確認します。サーバー全体の選択対象はありません。別のプロセスや別タブからの要求に依存せず、SSEなしでも単発APIを利用できます。

### 設定とプロセス一覧

- `GET /api/config`：パッケージのバージョン、起動時に指定した更新間隔、履歴の保持秒数を返します。
- `GET /api/processes`：要求ごとに `ProcessScanner` が `/proc` を走査し、プロセス識別子、名前、コマンド、ユーザー、メモリ量などを返します。前回の収集値との差分から CPU 使用率を求めます。初回など差分がない場合は null です。

### 要求時の観測・スレッド・マップ

次のAPIは識別子を検証し、要求ごとに `/proc` を読み取ります。SSE接続の保持状態は参照せず、ptraceによる停止も行いません。

- `GET /api/processes/observation`：CPU・RSS/VMS・fault・I/O・context switch・スレッドなどの単発観測を返します。比較対象となる前回観測を持たないため、CPU使用率と `rates` は null です。
- `GET /api/processes/threads`：単発観測の threads を返します。スレッドごとのCPU使用率は null です。
- `GET /api/processes/maps`：maps/smapsとrollupを読み、取得時刻・取得エラー・process_idとともに返します。取得前後に識別子を確認します。

継続的なCPU使用率と毎秒増分、履歴はSSEで取得します。通常の `/proc` 読み取りは対象を停止せず、各項目の取得時点は厳密には一致しません。

### FD と接続先

`GET /api/processes/fds` は識別子・生存を確認し、pipe/FIFO/socket の FD、アクセス方向、接続候補、同じリソースの共有所有者を収集します。UNIX domain socket の通信相手は socket diagnostic を使って調べます。TCP/UDP ソケットの通信相手の候補は、対象プロセスが属するネットワーク名前空間の情報から探索します。共有所有者と通信相手は区別し、データを消費する読み取りは行いません。探索は3秒・100,000 FD・一致8192 FDを上限とし、打ち切りなどを結果に含めます。

FD 情報の収集、通信相手の候補の照合、候補を所有するプロセス・FD の探索は [fds.rs](../src/http_server/process/fds.rs) を参照してください。UNIX domain socket の通信相手の inode を取得する処理は [socket diagnostic](../src/http_server/process/sockets.rs) にあります。

### 環境変数・補助ベクトル

いずれも識別子・生存を確認して要求時に取得します。定期観測に含めて再収集するものではありません。

これらの API は procfs に公開されたファイルを読み取ります。auxv の文字列参照先だけは、追加で `process_vm_readv` を使って対象プロセスのメモリから取得します。

- `GET /api/processes/environment`：procfs の [`/proc/<pid>/environ`](https://man7.org/linux/man-pages/man5/proc_pid_environ.5.html) を最大1 MiB読み取り、NUL 区切りの各項目を最初の `=` で名前と値に分けます。重複名・空値・値中の `=` を維持します。通常は exec 時の環境領域であり、起動後の変更すべてを反映しません。実装は [ファイルの読み取り](../src/http_server/process/details.rs) と [環境変数の解析](../src/http_server/process/details.rs) を参照してください。
- `GET /api/processes/auxv`：`/proc/<pid>/exe` の ELF ヘッダから32/64 bitを判別し、procfs の [`/proc/<pid>/auxv`](https://man7.org/linux/man-pages/man5/proc_pid_auxv.5.html) を最大64 KiB読み取ります。タグと値の組として解析し、既知・未知のタグを扱います。`AT_EXECFN`・`AT_PLATFORM`・`AT_BASE_PLATFORM` の文字列参照は、`process_vm_readv` で最大4096バイトまで解決します。参照先が読めなくても数値は保持します。big-endian ELF は対象外です。実装は [ELF ヘッダと auxv の読み取り](../src/http_server/process/details.rs)、[文字列参照の解決](../src/http_server/process/details.rs)、[メモリの読み取り](../src/http_server/process/memory.rs) を参照してください。

### 初回・定期スナップショットと非停止ライブサンプリング

各 SSE collector は初回 watch 値の配信前にスレッドを列挙し、通常の perf event を開いてから、各 TID を独立に `PTRACE_SEIZE` → `PTRACE_INTERRUPT` → `waitpid(__WALL)` で停止します。`PTRACE_GETREGSET(NT_PRSTATUS)` と `process_vm_readv` で同じ停止状態のレジスタと RSP から最大16 KiB（含有 mapping の末尾まで）のスタックを取得し、成功・失敗ともに detach します。初回・10秒ごとの ptrace と継続 perf は共通の `RawSample` と unwind・シンボル解決・レジスタ分類を使います。ptrace の CPU は null です。ptrace は初回に加え、取得完了から10秒後に全 TID を再取得します。perf で更新中の TID も対象です。遅延時の連続取得はせず、失敗しても次回試行は10秒後です。取得できない TID があっても前回サンプルを保持し、perf 監視を継続します。定期取得結果は通常の観測周期で配信します。

各 SSE collector は TID ごとに `perf_event_open(pid=tid, cpu=-1)` で software CPU-clock event を開き、CPU migration に追従します。初期周期は実行中の user CPU 時間10ms（約100Hz）、user stack dump は16384バイトです。待機遷移には追加の `PERF_COUNT_SW_CONTEXT_SWITCHES` event（周期1、stack dump 16384バイト）を使います。`context_switch` と `sample_id_all` を有効にし、同じ ring の次の SWITCH_OUT record の TID・CPU・時刻・PREEMPT flag を確認して voluntary / preempted を区別します。対応する sample がない場合や loss / throttle / switch-in の際は対応付けを破棄します。両 event の最新サンプルを monotonic 時刻で比較し、新しいものだけを採用します。TID/TIME/CPU/REGS_USER/STACK_USER を ring buffer から読み、最新値だけを保持します。約50msごとに drain と thread 追加・終了確認を行います。TID と開始時刻を確認して再利用を検出し、プロセス識別子も採取前後に検証します。

レジスタ18個とスタックは同じ perf sample または ptrace 停止状態に由来します。framehop は sampled RSP/RBP/RIP と採取済みバイトだけで最大256 frame を unwind します。Sampler ごとに module と rule cache を保持し、実行可能 mapping の変更時に再構築します。ELF の取得は既存の device/inode 検証とキャッシュを使い、PIE・非 PIE・共有ライブラリの load bias を補正します。`.eh_frame` / `.eh_frame_hdr`（header がなければ CFI index）と `.debug_frame` を読み、metadata が利用できない場合は framehop の frame-pointer fallback を使います。`.debug_frame` がある ELF はこれを優先し、CRT だけの `.eh_frame` に application の CFI が隠れることを防ぎます。シンボル・ソース・inline-frame 解決は既存の処理を再利用します。

`unwind_stop` は正常終了、256-frame 上限、stack snapshot の取得範囲不足、範囲外アドレス、module 取得失敗、module の metadata 不足を示します。後二者は fallback を使用した旨を添えます。section が存在しても個別の PC の CFI が欠ける／対応外の場合、framehop は内部で fallback するため、その詳細までは区別できません。unwind 中に live stack を追加取得したり ptrace stop したりしません。実行可能 mapping が変わるまで取得に失敗した module は再取得しません。

SSE の `live_samples` は TID、`sampled_at`（Unix ms）、`sample_age_ms`（monotonic clock）、CPU、`lost_samples`、registers、call_stack、disassembly、unwind_stop、error を含みます。未採取は時刻 null と Waiting for sample、採取済みの値は Threads 一覧に経過時間を表示します。sleeping/blocked thread も10秒ごとの ptrace で更新を試み、それ以外は最後の値を保持します。保存された user-space の状態を表示し、眠っている thread の現在値ではありません。観測開始前から眠っている thread も ptrace が許可されれば初回配信で採取できます。初回取得に失敗した場合は次の実行／switch-out または10秒後の ptrace 再試行を待ちます。Registers パネルにはレジスタ表だけを表示します。取得元や voluntary / preempted の区別は内部で保持し、GUI・HTTP API には公開しません。全スレッドの同時点状態は保証しません。perf 権限不足は Threads 一覧の thread error に表示します。collector エラーは API の `sampling_error` に保持し、通常観測は継続します。

Disassembly パネルには命令表と取得エラーを表示します。逆アセンブルは sampled RIP から `process_vm_readv` で後読みする best-effort 表示です。最大256バイト、最大32命令を iced-x86 で decode します。JIT/self-modifying code の命令バイトと sample 時点の RIP は整合しない場合があります。32-bit compatibility mode は対象外です。

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

購読側が遅延した場合は `gap` と最新の `full` snapshot を送り、失われた活動を再生しません。keep-alive は10秒間隔です。活動は約1秒ごとに集計・配信し、実際の集計期間は `window_ms` で送ります。

いずれのセンサーも CO-RE eBPF で実装しています。CPU scheduling・IPC・ファイル I/O は独立した eBPF プログラムで収集し、各センサーのロード・状態・解放も独立しています。SPACE は配信された `CpuActivity` を CPU の発光に使います。scheduler event はカーネルの map で集約し、userspace では活動の集計時に読み取ります。

| センサー | バックエンドの観測内容と制約 | eBPF ソース |
|---|---|---|
| CPU | `sched_switch` で実行時間と実行中 CPU を集計 | [sched.bpf.c](../src/http_server/system/sched.bpf.c) |
| IPC | pipe read/write と socket の送受信結果を観測。ペイロードは読まず、MSG_PEEK は加算しない。splice/sendfile、一部 io_uring、帰属不明のワーカーは対象外 | [ipc.bpf.c](../src/http_server/system/ipc.bpf.c) |
| ファイル I/O | VFS の read/write、ベクトル I/O の成功バイト数と回数を観測。ページキャッシュ経由も含む。mmap、io_uring、splice/sendfile、物理ディスク転送量は対象外 | [files.bpf.c](../src/http_server/system/files.bpf.c) |

ファイルのパスは操作時に取得し、取得できない場合は device/inode 等の識別子を使います。BPF のフックが利用できない場合はセンサーごとの理由を状態 API とログに出し、利用可能な情報の収集を継続します。必要なカーネル機能・権限はセンサーごとに異なります。

## フロントエンド側の処理

一覧・プロセス詳細・SPACE は独立した HTML と TypeScript の入口を持ち、通常のリンクで画面間を移動します。各画面の状態・接続はそのページで管理します。TypeScript は ES module として読み込み、どの画面もタイトルは `procinsh` です。

### 画面の入口とファイルの役割

| 場所・ファイル | 役割 |
|---|---|
| [list/index.html](../src/web/list/index.html)、[list/style.css](../src/web/list/style.css)、[list/app.ts](../src/web/list/app.ts) | `/`・`/list` の一覧。API 取得、検索、並べ替え、DOM 更新。`/list/app.js` を読み込む |
| [process/index.html](../src/web/process/index.html)、[process/style.css](../src/web/process/style.css)、[process/app.ts](../src/web/process/app.ts) | `/process/{pid}` の詳細。SSE 接続、DOM 更新、CPU/RSS 履歴の Canvas 描画、追加パネル。`/process/app.js` を読み込む |
| [space/index.html](../src/web/space/index.html)、[space/style.css](../src/web/space/style.css)、[space/app.ts](../src/web/space/app.ts) | `/space` の Canvas、検索・操作ボタン、選択詳細、システム SSE、Three.js 描画。`/space/app.js` を読み込む |
| [space/model.ts](../src/web/space/model.ts) | Three.js に依存しない構造イベントのマージ、配置、活動表示のモデル、描画解像度の調整 |
| [shared/api.ts](../src/web/shared/api.ts) | 一覧・詳細で共有する JSON 取得と API エラーの扱い |
| [shared/display.ts](../src/web/shared/display.ts) | バイト数・割合・毎秒増分、権限・スケジューラ・ソケットなどの表示形式 |
| [shared/dom.ts](../src/web/shared/dom.ts)、[shared/style.css](../src/web/shared/style.css) | 一覧・詳細の要素生成と、ヘッダー・フォーム・テーブルなどの共通スタイル |
| [shared/navigation.ts](../src/web/shared/navigation.ts) | プロセス識別子の照合、API クエリ、開始時刻を引き継ぐ詳細リンク |
| [shared/api-types.ts](../src/web/shared/api-types.ts) | Rust の JSON/SSE 応答に対応する TypeScript 型。実行時の入力検証は行わない |
| [list/dom-types.ts](../src/web/list/dom-types.ts)、[process/dom-types.ts](../src/web/process/dom-types.ts)、[space/dom-types.ts](../src/web/space/dom-types.ts) | 各 HTML の要素 ID と要素型の対応。`ListElements`・`ProcessElements`・`SpaceElements` |
| [vendor/](../src/web/vendor/) | 同梱の Three.js と、カメラ操作を担当する [OrbitControls.js](../src/web/vendor/OrbitControls.js) |
| [web.rs](../src/http_server/web.rs) | HTML/CSS、生成済み JavaScript、vendor ファイルを Rust バイナリに埋め込み、各 URL で配信 |

TypeScript のビルド設定は [tsconfig.json](../tsconfig.json)、実行コマンドは [package.json](../package.json) にあります。生成 JS も `dist/web/` 内で画面別・共有のディレクトリに分かれ、JS/CSS の配信 URL は `/list/`・`/process/`・`/space/`・`/shared/` 配下です。変更した画面を確認するには、[ビルドと実行環境](#ビルドと実行環境)の手順で Web と Rust バイナリをビルドします。

### プロセス一覧 `/list`（`/` も同じ一覧を表示）

実装の入口は [list/app.ts の `start`](../src/web/list/app.ts#L97)、定期取得は [`refresh`](../src/web/list/app.ts#L25)、一覧の DOM 更新は [`renderProcesses`](../src/web/list/app.ts#L42) です。

一覧ページは一覧の DOM だけを持ち、詳細 SSE を開きません。

| 利用API | 呼び出すタイミングと用途 |
|---|---|
| `GET /api/config` | 初期化・ページキャッシュからの復帰時に更新間隔を取得 |
| `GET /api/processes` | 一覧表示時と定期更新時に一覧を取得 |

一覧の更新間隔は設定値と1000msの大きい方です。一覧の取得中は次の取得をスキップします。PID・名前・コマンドラインの検索と、CPU・RSS の降順または PID の昇順の並べ替えは、取得済みデータに対してブラウザ内で行います。

プロセス名は [`processUrl`](../src/web/shared/navigation.ts#L4) が生成する `/process/{pid}?start_time_ticks={開始時刻}` へのリンクです。通常のページ遷移で詳細を開き、観測した `{pid, start_time_ticks}` を引き継ぎます。ヘッダーの Go to space view は `/space` へ移動します。

`pagehide` で定期更新を停止し、取得中の一覧要求をキャンセルします。ページキャッシュからの復帰時は設定と一覧を取得し直し、定期更新を再開します。

### プロセス詳細 `/process/{pid}`

SSE の接続は [process/app.ts の `connect`](../src/web/process/app.ts#L234)、受信状態の反映は [`acceptTarget`](../src/web/process/app.ts#L284) と [`renderTarget`](../src/web/process/app.ts#L302) が担当します。追加パネルの取得は [`loadProcessDetails`](../src/web/process/app.ts#L66)、履歴グラフは [`drawHistory`](../src/web/process/app.ts#L415)、選択スレッドのサンプル表示は [`renderLiveSample`](../src/web/process/app.ts#L523) を参照してください。

| 利用API | 呼び出すタイミングと用途 |
|---|---|
| `GET /api/processes` | 初期化時に PID と、URL で指定された開始時刻を照合。開始時刻がない URL では PID の開始時刻を解決 |
| `GET /api/processes/events` | 識別子クエリを付けて接続し、概要・観測・履歴・スレッド・マップ・終了状態を更新 |
| `GET /api/processes/environment`、`GET /api/processes/auxv`、`GET /api/processes/fds` | 各パネルを開くときと、開いている間は5秒ごと（取得中はスキップ） |

詳細ページは詳細の DOM だけを持ち、一覧の定期更新は行いません。起動時に一覧 API を一度取得し、PID と URL の `start_time_ticks` を照合して SSE を接続します。開始時刻がない URL では現在の識別子を解決し、URL に開始時刻を保持します。開始時刻が不正、対象不在、識別子が不一致の場合は詳細ページでエラーを表示し、一覧へのリンクを残します。識別子が不一致のプロセスには接続しません。追加 GET にも同じ識別子クエリを付けます。通常の更新には SSE の `observation` を使い、observation・threads・maps の単発 GET は呼びません。

受信した概要・観測からメトリクスとスレッド一覧を更新し、履歴から CPU/RSS のグラフを Canvas に描画します。マップは取得時刻の変更または取得エラーに応じて表示を更新します。スレッド一覧には sample age、未採取なら Waiting for sample、取得エラーがあればその内容を表示します。

各スレッドの最新サンプルを保持し、選択したスレッドのレジスタ・Call Stack・逆アセンブルを表示します。Call Stack は詳細グリッドの全列にまたがり、フレームのアドレス、シンボル、ソース位置、inline frame、unwind の停止理由やエラーを表示します。マップ・レジスタ・スタック・逆アセンブル・auxv のアドレスと、環境変数などの文字列は `textContent` で表示します。

Environment・Auxiliary Vector・Pipe / Socket はパネルを開くたびに単発 GET で取得し、開いているパネルだけ5秒ごとに更新します。同種の取得中は次の更新をスキップします。Environment と Pipe / Socket の検索は取得済みデータに対して行います。再取得が失敗した場合は、以前の結果があれば残したうえでエラーを表示します。

一覧へのリンク、ヘッダーのアプリ名、FD の接続先リンクは通常のページ遷移です。FD のリンクにも観測した開始時刻を引き継ぎます。各詳細ページは独立した詳細情報とパネルの開閉・検索状態を持ちます。接続の世代と識別子を照合して古い SSE 通知を無視し、追加 GET は要求時の世代と識別子、応答の識別子を照合して古い応答を破棄します。プロセス終了・`pagehide` では追加パネルの定期更新を停止し、取得途中の応答も無効化します。

通信切断ではエラーを表示し、EventSource が同じ識別子で自動再接続します。再接続後の観測を受信するとエラーを消し、履歴は新しい接続で再開始します。同じ PID の別プロセスへ自動で乗り換えません。`exited: true` を受信したら接続を閉じ、最終状態と終了表示を残します。`pagehide` で接続を閉じ、ページキャッシュからの `pageshow` では終了していない対象に同じ識別子で接続し直します。

### SPACE `/space`

SPACE はシステム全体のプロセス、仮想アドレス空間、親子関係、IPC・ネットワーク接続、ファイル I/O を3D表示します。SSE と DOM・Three.js の操作は [space/app.ts](../src/web/space/app.ts)、受信データのマージや座標・活動の計算は [space/model.ts](../src/web/space/model.ts) が担当します。

#### データの受信と構造の更新

表示開始・再表示時に [`start`](../src/web/space/app.ts#L1391) が `GET /api/system/events` に EventSource で接続します。初期構造も SSE から取得し、接続中だけバックエンドの閲覧者として登録されます。イベントの型は [api-types.ts の `SystemSnapshotUpdate`](../src/web/shared/api-types.ts#L253)・[`SpaceActivity`](../src/web/shared/api-types.ts#L279)、配信内容は [HTTP API の説明](#get-apisystemevents)を参照してください。

| イベント | 反映する内容 | 主な実装 |
|---|---|---|
| `snapshot` | `full` は構造を置き換え、`delta` は保持済みの構造へマージ。差分の `base_sequence` と保持済みの `sequence` を照合し、maps と FD 関係の差分を適用。省略された maps は同じプロセス識別子の前回値を保持 | [`mergeSnapshot`](../src/web/space/model.ts#L532) → [`rebuild`](../src/web/space/app.ts#L404) → [`buildScene`](../src/web/space/app.ts#L450) |
| `activity` | CPU の発光、IPC・ネットワークの粒子、最近のファイル I/O の表示を更新 | [`activity`](../src/web/space/app.ts#L1115) |
| `gap` | 描画中の粒子をクリアし、続く `full` snapshot を反映 | [`start` のイベントハンドラ](../src/web/space/app.ts#L1391) |

#### プロセス・接続先・ファイルの配置

[`rebuild`](../src/web/space/app.ts#L404) が受信構造から描画用の状態を作り、[`buildScene`](../src/web/space/app.ts#L450) がプロセスの箱・メモリ領域・親子線・接続線を構築します。構造更新では既存のプロセスと接続先の位置を維持しながら、追加・削除を反映します。

| 対象 | 配置・表示の考え方 | 主な実装 |
|---|---|---|
| プロセスと親子関係 | 親子関係に沿って平面に配置。通常の更新は既存位置を保ち、新規プロセスを空き領域へ配置。Rearrange は全体を再配置 | [`treeLayout`](../src/web/space/model.ts#L239)、[`stableLayout`](../src/web/space/model.ts#L356) |
| 仮想アドレス空間 | アドレス順にメモリ領域を積み上げ、アドレスの隙間を圧縮し、高さをプロセスごとに正規化。プロセス間の同じ高さは同じアドレスを意味しない | [`layoutMaps`](../src/web/space/model.ts#L164)、[`regionColor`](../src/web/space/app.ts#L343) |
| ユーザーの識別 | 箱の上下の枠は実 UID、縦の枠は実効 UID に応じて色分け | [`userColor`・`processColors`](../src/web/space/model.ts#L429) |
| IPC・ネットワーク接続 | FD 関係を線で表示。接続候補・共有 FD は破線。ネットワーク接続先はプロセス・プロトコル・相手 IP/port ごとにまとめ、プロセスの上方に配置 | [`buildScene`](../src/web/space/app.ts#L450)、[`networkGroups`](../src/web/space/model.ts#L94)、[`networkLayout`](../src/web/space/model.ts#L114) |
| 最近アクセスしたファイル | プロセスの下方にファイルのマーカーと接続線を配置。構造とは別のグループで更新 | [`fileLayout`](../src/web/space/model.ts#L512)、[`refreshFileScene`](../src/web/space/app.ts#L812) |

#### 活動の表示と描画ループ

[`activity`](../src/web/space/app.ts#L1115) が活動データを保持し、[`animate`](../src/web/space/app.ts#L1308) が `requestAnimationFrame` ごとに粒子と CPU の発光を更新します。タブ非表示中は描画処理をスキップします。

| 表示 | 振る舞い | 主な実装 |
|---|---|---|
| CPU の発光 | 実行中に強まり、活動が途絶えると約500msで減衰 | [`cpuGlowLevel`](../src/web/space/model.ts#L221) |
| IPC・ネットワークの粒子 | 読み書きの向きと操作回数に応じて粒子を生成。接続先を一つに特定できない場合は操作元のポートだけを発光。`prefers-reduced-motion` に応じて粒子の表示時間と数を減らす | [`edgeDirection`](../src/web/space/model.ts#L183)、[`ipcParticlePlan`](../src/web/space/model.ts#L202)、[`activity`](../src/web/space/app.ts#L1115) |
| ファイル I/O | 読み書きの粒子と、保持中のファイルのバイト数・操作回数を表示。最終アクセスから30秒、各プロセス32個・全体512個まで保持し、終了したプロセスのファイルは削除 | [`RecentFiles`](../src/web/space/model.ts#L449)、[`fileDetails`](../src/web/space/app.ts#L888) |
| プロセス名・接続先ラベル | 3D 座標を画面座標へ投影し、WebGL とは別の Canvas 2D に描画 | [`drawLabels`](../src/web/space/app.ts#L253) |

描画解像度は [`AdaptiveRenderScale`](../src/web/space/model.ts#L18) が約1秒ごとの FPS で調整します。24 FPS 未満が3回続いた場合は pixel ratio を10%下げ、45 FPS 以上が5回続いた場合は元の解像度に向けて回復します。上限は初期の device pixel ratio（最大1.5）、下限は0.5（初期値が0.5未満ならその値）です。FPS と解像度の割合はヘッダーに表示します。

#### 検索・選択とカメラ操作

操作はブラウザ内で処理します。選択時に詳細 API は呼ばず、取得済みの構造・活動データから右側の詳細パネルを更新します。Open process details のリンクは [`processUrl`](../src/web/shared/navigation.ts#L4) で観測した開始時刻を引き継ぎ、`/process/{pid}?start_time_ticks={開始時刻}` に移動します。

| 操作 | 振る舞い | 主な実装 |
|---|---|---|
| PID・プロセス名の検索 | 描画対象を絞り込み、Enter で最初の候補を選択してカメラを寄せる | [`visibleIds`](../src/web/space/app.ts#L387)、[検索イベント](../src/web/space/app.ts#L1292) |
| クリック・ホバー | プロセス、接続線、ネットワーク接続先、ファイルを選択・説明表示。プロセスのダブルクリックでカメラを寄せる | [`hit`・`edgeHit`](../src/web/space/app.ts#L1187)、[pointer イベント](../src/web/space/app.ts#L1229)、[`details`](../src/web/space/app.ts#L972) |
| ドラッグ・右ドラッグ・スクロール | カメラの回転・平行移動・ズーム | [OrbitControls の設定](../src/web/space/app.ts#L135) |
| Fit all | 検索と選択を解除し、全体が見えるようにカメラを調整 | [`fit`](../src/web/space/app.ts#L376)、[ボタンイベント](../src/web/space/app.ts#L1286) |
| Rearrange | 粒子をクリアし、プロセス・ネットワーク接続先・ファイルを再配置して全体を表示 | [`rebuild`](../src/web/space/app.ts#L404)、[ボタンイベント](../src/web/space/app.ts#L1280) |

#### 接続とページのライフサイクル

接続管理は [`start`・`stop`](../src/web/space/app.ts#L1391)、ページイベントは [`visibilitychange`・`pagehide`・`pageshow` のハンドラ](../src/web/space/app.ts#L1440) を参照してください。

- タブ非表示・`pagehide` では SSE と再接続タイマーを止め、粒子・CPU の発光・ファイル表示をクリアします。タブの再表示やページキャッシュからの復帰時は接続し直します。
- 接続エラーでは現在の EventSource を閉じ、エラーを表示して、表示中に限り3秒後に新しい接続を作ります。接続成功時にエラー表示を消し、古い接続からのイベントは無視します。
- 構造イベントの解析やマージに失敗した場合は接続を閉じ、表示中なら直ちに接続し直して初期構造を取得します。
- タブの表示状態の変更・ページ離脱・ページキャッシュからの復帰時には、FPS の計測と解像度調整の連続回数をリセットします。

#### SPACE の検証入口

| 対象 | テストソース | 実行手順 |
|---|---|---|
| データのマージ、配置、活動モデル、解像度調整 | [space-model.mjs](../tests/space-model.mjs) | `npm run build:web` 後に `node tests/space-model.mjs` |
| WebGL 描画、配置・選択・カメラ操作、接続管理 | [space-browser.mjs](../tests/space-browser.mjs)。ネットワークとファイルは [space-network.mjs](../tests/space-network.mjs)・[space-files.mjs](../tests/space-files.mjs) を呼び出して検証 | [ブラウザテスト](#ブラウザテスト)の準備後に `node tests/space-browser.mjs` |
| 実機の CPU・IPC・ファイル I/O センサー | [space-live.py](../tests/space-live.py)、[space-files-live.py](../tests/space-files-live.py) | [実機センサーテスト](#実機センサーテスト)を参照 |

## 権限

`process_vm_readv` は所有者、dumpable 属性、Yama、`CAP_SYS_PTRACE`、seccomp などの制約を受けます。BPF はカーネル側の対応と観測権限も必要です。権限やカーネル設定の自動変更、sudo の自動実行はしません。

待受の既定値は `127.0.0.1:8080` です。IPv4/IPv6 の loopback（`127.0.0.1`、`::1` など）は追加フラグなしで利用できます。LAN アドレスや wildcard（`0.0.0.0`、`::`）など非 loopback の指定は、`--allow-non-loopback` がなければ bind 前に非ゼロ終了します。明示的に許可する例は `procinsh --listen 0.0.0.0:9090 --allow-non-loopback` です。許可した場合も警告を出します。

認証・TLS はありません。接続できる利用者はプロセスメモリや環境変数にアクセスできるため、非 loopback での待受はアクセス範囲を管理した信頼できるネットワーク内に限定してください。Host/Origin/Fetch Metadata の検証と API レスポンスの `Cache-Control: no-store` は維持しますが、これらは認証の代わりにはなりません。

## ログ

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

ビルドと fixture の準備後に実行します。

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

`./scripts/dev_test.sh [テスト名フィルター] [--nocapture]` は単体テストをビルドし、既存の sudoers で許可された実行パスに一時配置して capability を付けます。テスト成功・失敗のいずれでも元のアプリケーションバイナリを復元します。権限設定と実行は `dev_run.sh` に委譲します。この実行中は同じ checkout でビルドや dev_run を並行実行しないでください。

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

### Chrome による通信・IPC の負荷生成

`scripts/chrome_load.mjs` は Google Chrome のタブを約10秒ごとにキャッシュを無視してリロードします。Node.js 22以降と Chrome が必要で、追加の npm パッケージは不要です。標準では `scripts/chrome_sites.txt` の先頭20サイトを開きます。GPU を無効にして実行し、画面セッション（`DISPLAY` / `WAYLAND_DISPLAY`）がない場合はヘッドレスで起動します。

```sh
node scripts/chrome_load.mjs

# 100サイト、10秒間隔
TAB_COUNT=100 INTERVAL_SECONDS=10 node scripts/chrome_load.mjs

# Chrome の実行ファイルと観測する URL を指定
CHROME=/usr/bin/chromium node scripts/chrome_load.mjs http://127.0.0.1:8080/

# 独自のサイト一覧（1行1 URL、空行と # から始まる行は無視）
SITE_LIST=/path/to/sites.txt TAB_COUNT=5 node scripts/chrome_load.mjs
```

`TAB_COUNT` は1〜1000、`INTERVAL_SECONDS` は1秒以上です。タブ数が URL 数を超える場合は一覧を繰り返します。サイトによって読み込み時間、リダイレクト、アクセス制限が異なるため、ログのリロード完了は DevTools コマンドの成功を表し、すべてのコンテンツの読み込み完了は保証しません。

Chrome は独立した一時プロファイルと loopback の DevTools ポートを使い、起動したコントローラーと Chrome の PID をログに出します。Ctrl+C またはコントローラーへの SIGTERM で Chrome を停止して一時プロファイルを削除します。`PID_FILE=/path/to/load.pid` を指定するとコントローラーの PID を保存し、終了時に削除します。既存の PID ファイルは上書きしません。

対話セッション終了後も継続する場合は tmux で起動できます。下記はリポジトリのルートで実行します。

```sh
tmux new-session -d -s procinsh-load \
  "exec env PID_FILE=/tmp/procinsh-chrome-load.pid node '$PWD/scripts/chrome_load.mjs' >> /tmp/procinsh-chrome-load.log 2>&1"
tail -f /tmp/procinsh-chrome-load.log

# 負荷生成を停止
kill -TERM "$(cat /tmp/procinsh-chrome-load.pid)"
```

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
