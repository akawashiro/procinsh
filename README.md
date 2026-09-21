# procinsh

Linux x86-64 用の、ローカル Web ベースのプロセスインスペクタです。PLAN.md の **v0.1.0 / M0–M5** を実装しています。

## 起動

Rust stable（検証環境: 1.95）と Linux x86-64 が必要です。

```sh
cargo build --release --locked
./target/release/procinsh
```

ブラウザで **http://127.0.0.1:8080** を開きます。HTML / CSS / JavaScript はバイナリに埋め込まれており、Node.js や外部 CDN は実行時に不要です。

```sh
./target/release/procinsh --pid 12345
./target/release/procinsh --pid 12345 --interval 250ms
./target/release/procinsh --listen 127.0.0.1:9090
```

必要なら `cargo install --path . --locked` で `procinsh` コマンドとしてインストールできます。更新間隔は **100ms～60s、標準1s**。履歴は直近60秒、smaps は選択中のプロセスのみ約5秒間隔で取得します。

## 操作

1. Process Explorer で名前・PID・コマンドを検索し、CPU / RSS / PID で並べ替えます。
2. プロセス名をクリックすると、CPU・RSS / VMS・fault・context switch・I/O・全スレッドのライブ表示が開きます。
3. スレッドを選択し **Coherent Snapshot を取得** を押すと、全スレッドを一時停止してレジスタ・マップ・frame pointer のスタックを取得します。
4. レジスタやマップのアドレスをクリックすると、その時点のメモリを hex / ASCII で読み取ります。アドレスを手入力することもできます。標準256バイト、最大64 KiBです。
5. **← Processes** で一覧に戻り、別のプロセスへ切り替えます。

Registers の **自動取得（1秒）** を ON にすると即座に取得し、その後は1秒ごとにレジスタと Call Stack を更新します。初期状態は OFF です。取得中は次の回をスキップし、同じタブからの取得を重ねません。選択中のスレッドを維持し、最新の観測とスナップショットの両方から消えた場合は生存中の先頭スレッドへ移ります。

OFF にすると次回以降を停止します。実行中の取得は停止解除まで完了させ、同じ対象なら最後の結果を表示します。対象切り替え、一覧へ戻る操作、対象終了、タブ非表示、取得・接続エラーでも OFF になり、自動再開はしません。エラー時は最後に成功した結果を保持します。設定はタブごとで保存されず、ブラウザのタイマーのため厳密な毎秒実行ではありません。**毎回、対象の全スレッドを一時停止します**。通常の観測間隔 `--interval` とは別の設定で、ptrace 権限は手動取得と同様に必要です。

CPU は1コアを100%とするため、マルチスレッドでは100%を超えます。最初の観測では差分を計算できないため CPU / rate は N/A です。プロセスの context switch は生存中のスレッドを合計し、rate は同じ TID + starttime のスレッドの差分から算出します。観測間に終了したスレッドの最後の差分は含められません。username は `/etc/passwd` を参照し、取得できない場合は UID を表示します。

詳細監視の対象はサーバー全体で常に1つです。複数タブも同じ選択を共有します。識別子は **PID + `/proc/PID/stat` の starttime**。終了した対象を同じ PID の別プロセスへ自動接続しません。

通常の `/proc` 観測は非停止で、各フィールドは厳密には同時点ではありません。スナップショットは取得時刻と経過時間を表示します。Memory Viewer はスナップショットとは別時点の現在メモリです。partial read は取得できたバイトだけ表示します。

## Pipe / Socket と接続先 PID

詳細画面の **Pipe / Socket · 接続先プロセス** を開くと、対象の pipe・FIFO・socket の FD 番号、inode、読み書き方向、プロトコル、状態、ローカル／リモートアドレスを表示します。FD / PID / 名前 / アドレスで検索でき、「再取得」で更新します。**相手の PID をクリックすると、そのプロセスの Inspector へ直接移動**します。PID と starttime を渡すため、相手が終了・再利用された場合は別プロセスへ接続しません。

- pipe / FIFO: 同じ inode（FIFO は device も一致）を持つ FD を探索し、読み書き方向が対応するものを相手として表示します。同じ方向または方向不明の FD は共有者として別枠に表示します。
- UNIX socket: `NETLINK_SOCK_DIAG` の `UNIX_DIAG_PEER` で実際の接続先 inode を取得し、その FD を持つ PID を探します。同じ socket を fork / dup 等で共有するプロセスは接続先と区別します。
- TCP / UDP（IPv4 / IPv6）: 対象の network namespace の `/proc/PID/net/` を参照し、逆向きのアドレス・ポートが一致するローカル socket を候補として表示します。待受 socket を接続済みの相手と混同しません。UDP の候補は永続的な接続関係を保証しません。

複数の相手 FD が見つかる場合は列挙します。リモートホスト、未接続、終了済み、アクセス権不足等で PID が分からない場合も、取得できたアドレス等は表示します。UNIX socket の peer 取得は inspector と同じ network namespace に限定し、異なる namespace へ入るための権限は追加しません。他の socket family は inode / 共有者を表示し、接続先が不明ならその旨を表示します。

この一覧は非停止の一時点の探索結果で、取得中にも FD は開閉されます。全プロセスの FD を探索するため自動更新には含めず、探索予算3秒・最大100,000 FD・一致8192 FDを上限とし、到達時や権限不足時は不完全な一覧であることを表示します。対象の一覧は最大4096項目、各行の相手・共有者の表示にも上限を設けます。pipe / socket の内容を読み取ったり消費したりはしません。

参照: [proc_pid_fd(5)](https://www.man7.org/linux/man-pages/man5/proc_pid_fd.5.html)、[sock_diag(7)](https://www.man7.org/linux/man-pages/man7/sock_diag.7.html)。

## 環境変数と補助ベクトル

詳細画面下部の **Environment · 環境変数**、**Auxiliary Vector · 補助ベクトル** を開くと、それぞれ `/proc/PID/environ`、`/proc/PID/auxv` を読み取ります。必要なときだけ取得し、「再取得」で更新します。SSE やスナップショットの自動取得には含めません。取得時刻を表示し、対象切り替え時に結果を消去します。

環境変数は名前・値で検索できます。`=` を含む値、空の値、同名の複数エントリを維持し、HTML として解釈せず文字として表示します。不正な UTF-8 は置換文字にして注記します。`environ` は通常 exec 時の環境領域であり、起動後の `setenv()` 等による変更を完全には反映しません。取得上限は1 MiBです。

auxv は ELF の word size（32 / 64 bit）を確認し、`AT_ENTRY`、`AT_PHDR`、`AT_BASE`、`AT_PAGESZ`、`AT_UID`、`AT_SECURE`、`AT_HWCAP`、`AT_RANDOM`、`AT_EXECFN`、`AT_SYSINFO_EHDR` 等を名前・説明・16進値・10進値で表示します。未知のタグも値を保持します。アドレスは Memory Viewer にリンクし、`AT_EXECFN` / `AT_PLATFORM` / `AT_BASE_PLATFORM` の文字列は取得時のメモリから最大4096バイトまで読み取ります。文字列だけが読めない場合も auxv の数値は表示します。auxv の取得上限は64 KiBで、big-endian ELF は対象外です。

どちらも非停止の読み取りで、PID + starttime を検証します。権限不足は各パネルに表示し、通常監視は継続します。環境変数には秘密情報も含まれ得るため、既存のメモリ閲覧と同じアクセス権限で扱います。

参照: [proc_pid_environ(5)](https://man7.org/linux/man-pages/man5/proc_pid_environ.5.html)、[proc_pid_auxv(5)](https://www.man7.org/linux/man-pages/man5/proc_pid_auxv.5.html)、[getauxval(3)](https://www.man7.org/linux/man-pages/man3/getauxval.3.html)。

## スタックとシンボル

**Disassembly** パネルは選択スレッドの RIP から最大32命令を、x86-64 / Intel 構文で表示します。アドレス、命令バイト、逆アセンブル結果を並べ、RIP の行を強調します。手動・自動スナップショットとスレッド選択に連動し、取得時刻と経過時間も表示します。

命令バイトはレジスタと同じ停止中に `process_vm_readv` で最大256バイト（RIP が含まれるマップの末尾まで）取得し、対象の再開後に `iced-x86` でデコードします。JIT や変更済みコードも実メモリの取得結果を使います。命令境界が確定している RIP を起点に前方のみを表示し、過去の実行履歴や分岐後の実行順は示しません。読み取り失敗、途中で切れた命令、無効な命令はパネル内に表示し、レジスタやスタックの取得結果は維持します。32-bit compatibility mode は対象外です。アドレスをクリックすると、別時点の現在メモリを Memory Viewer で開きます。

デバッグ情報と frame pointer を有効にして対象をビルドしてください。

```sh
# C / C++
cc -g -fno-omit-frame-pointer -fno-optimize-sibling-calls target.c -o target

# Rust
RUSTFLAGS="-C force-frame-pointers=yes" cargo build
```

ELF の segment offset を使って PIE / ASLR を解決し、関数名・関数内オフセット・ソースファイル・行・inline frame を表示します。解析結果はファイルの device / inode / size / mtime でキャッシュします。読み込めない ELF / DWARF は生アドレスにフォールバックします。

スタックは最大256フレーム。RBP の整列、範囲、単調増加、実行可能な戻り先を検証し、停止理由を表示します。frame pointer が省略されたライブラリ、signal trampoline、最適化された任意バイナリの完全な unwind は対象外です。対象の実行ファイルが削除済みで `/proc/PID/map_files` 等にもアクセスできない場合はシンボルを解決できません。

スナップショットは `PTRACE_SEIZE` → `PTRACE_INTERRUPT` → 全スレッドの停止確認後に取得します。追加スレッドを再列挙し、4096スレッド / 16回の安定化試行 / 停止待ち2秒を上限とします。取得にも2秒の処理予算を設けています（カーネル内でブロックした syscall の実時間の上限は保証できません）。RAII で detach を試み、専用 OS スレッドを終了して残った tracee のカーネルによる detach も確保します。対象の再開後にシンボルを解決します。既存の job-control stop と signal delivery を維持し、inspector 自身のスナップショットは拒否します。

## 権限とアクセス

`ptrace` / `process_vm_readv` はプロセス所有者、dumpable 属性、Yama `kernel.yama.ptrace_scope`、`CAP_SYS_PTRACE`、seccomp 等に制約されます。失敗しても通常監視は継続でき、UI に原因候補を表示します。権限やカーネル設定を自動変更せず、sudo も自動実行しません。

通常は `127.0.0.1` のみに bind します。Host / Origin / Fetch Metadata を検証し、別サイトからのリクエストを拒否します。メモリレスポンスはキャッシュしません。

```sh
# 明示的な外部公開（認証機能なし）
./target/release/procinsh --listen 0.0.0.0:8080
```

外部公開時は数値IPでアクセスしてください。プロセスのメモリには秘密情報が含まれるため、信頼できるネットワーク内でのみ利用してください。

## API

| Method / path | 内容 |
|---|---|
| `GET /api/config` | バージョン、更新間隔 |
| `GET /api/processes` | 軽量なプロセス一覧 |
| `GET /api/target` | 選択対象、最新観測、履歴、マップ（未選択は null） |
| `POST /api/target` | JSON の `{ "pid": 123, "start_time_ticks": 456 }` で選択 |
| `DELETE /api/target` | 同じ識別子の JSON で選択解除 |
| `GET /api/target/process` | 最新の ProcessObservation |
| `GET /api/target/threads` | 全スレッドの観測 |
| `GET /api/target/maps` | maps / smaps と rollup、取得時刻 |
| `GET /api/target/memory` | `address=0x...&length=256` のメモリ |
| `GET /api/target/fds` | pipe / FIFO / socket と接続先・共有者の PID + starttime |
| `GET /api/target/environment` | 環境変数の名前・値、取得時刻 |
| `GET /api/target/auxv` | 補助ベクトルの名前・値・文字列、取得時刻 |
| `POST /api/target/snapshot` | 識別子の JSON を渡して coherent snapshot |
| `GET /api/target/events` | SSE の `observation` イベント。再接続時は最新状態を送信 |

`process` / `threads` / `maps` / `memory` / `environment` / `auxv` / `fds` の GET には、一覧から取得した `pid` と `start_time_ticks` をクエリに含めてください。選択の切り替えと並行した古いリクエストは409、対象終了は410、無効なメモリ範囲は400です。アドレスは JavaScript の整数精度を維持するため **16進文字列**で返します。

## 検証

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked

# ブラウザ操作の結合テスト（Node.js 22+ / Google Chrome）
cargo build
sh tests/targets/build.sh
node tests/browser.mjs
# Chrome のパスが異なる場合
CHROME=/usr/bin/chromium node tests/browser.mjs
```

Rust の結合テストは `cc` で専用の子プロセスをビルド・起動します。テスト環境は `ptrace` と `process_vm_readv` を許可している必要があります。ブラウザテストは一時プロファイルとテスト対象だけを起動し、完了時に終了します。画面は `target/browser-inspector.png` に保存します。

`tests/targets/` に busy_loop / sleeping / threads（スレッド増減を含む）/ allocator / recursive / mmap_test / ipc（pipe・UNIX・TCP・UDP の親子プロセス）があります。手動確認には以下を使えます。

```sh
sh tests/targets/build.sh
tests/targets/bin/recursive --allow-inspector
```

テスト専用の `--allow-inspector` は、この fixture だけを同じユーザーの sibling inspector から観測可能にします。カーネル全体の Yama 設定は変更しません。fixture は120秒で自動終了します。

## 今後の範囲

PLAN.md に従い、continuous perf sampling、sampled register / stack、集約、flame graph は v0.2.0 以降です。`/api/target/sample` は501を返します。`--sample-frequency`、`-- ./target`、DWARF unwind、メモリ／レジスタ書き換え、他OS／ARMは実装していません。

実装時の参照: [Linux ptrace](https://man7.org/linux/man-pages/man2/ptrace.2.html)、[addr2line Loader](https://docs.rs/addr2line/0.24.2/addr2line/struct.Loader.html)。

### シグナル情報

プロセス詳細の「Signals · シグナル」を開くと `/proc/PID/status` と
`/proc/PID/task/TID/status` を読み取ります。「再取得」で更新できます。
プロセス共有の保留 (`ShdPnd`)、無視 (`SigIgn`)、ハンドラ登録 (`SigCgt`)、
各スレッドの保留 (`SigPnd`) とブロック (`SigBlk`) を16進マスクとシグナル名で表示します。
`SigQ` は対象の実 UID 全体のキュー数と対象プロセスのリソース上限で、対象プロセスだけの件数ではありません。
リアルタイムシグナルは Linux カーネルの番号32–64を表示し、libc の `SIGRTMIN` は推測しません。
非停止の逐次読み取りのためスレッド間で取得時刻は異なり、受信履歴・送信元・ハンドラアドレスは含みません。
シグナルの送信や設定変更は行いません。最大4096スレッド・2秒で打ち切り、取得失敗は表示します。
API: `GET /api/target/signals?pid=PID&start_time_ticks=START_TIME`（選択中のプロセス識別子が必要）。

フィールドの意味: [proc_pid_status(5)](https://man7.org/linux/man-pages/man5/proc_pid_status.5.html)。

### SPACE: 全プロセスの3D観測

`/space`（一覧の **ENTER SPACE**）は全プロセスの仮想アドレス空間と
pipe/socket 接続と通常ファイルの読み書きを表示します。ドラッグで回転、右ドラッグでパン、ホイールでズーム。
プロセスは親子関係の階層ツリーに配置し、地面付近の薄い線で親子を結びます。
直方体を選択すると祖先と直接の子を強調し、ダブルクリックでフォーカスします。
検索欄の Enter でもフォーカスできます。通信ケーブルのホバーで接続種別と直近の観測量、
クリックで両端のプロセスとFDを表示します。

Z 軸はアドレス順で、マッピング内部は線形、隙間は圧縮します。
各プロセスの高さは正規化しており、プロセス間の同じ高さは同じアドレスではありません。
遠くの領域は最大64層にまとめて描画します。
TCP/UDP の候補と共有 FD は破線で区別します。同じプロセスの重複 FD はまとめ、
共有所有者は代表との線で表現して全組合せの線を作りません。複数所有者などで相手を一意に
絞れない通信は、実行したプロセスのポートだけが発光します。

発光の根拠は実測のみです。

- CPU: CO-RE eBPF の `sched_switch` で、各スレッドが実際に CPU 上で実行された時間と
  現在実行中の CPU をプロセスごとに集計します。実行中は直方体の底面が強く発光し、
  CPU から離れた後は約500msで減衰します。実行可能状態や約1秒の CPU 使用率は発光の根拠にしません。
- IPC: CO-RE eBPF の fexit で pipe の read/write、`sock_send_length` /
  `sock_recv_length` トレースポイントで socket の送受信結果を観測。MSG_PEEK は通信量に含めません。
  FIFO ラッパーは匿名 pipe 関数を呼ぶため重複フックを置きません。
  ペイロードを読み取らず、操作時点の inode/device とプロセス開始時刻で帰属を検証します。
  splice/sendfile と一部の io_uring 経路は網羅しません。カーネル/IO ワーカーは帰属不明として除外します。
- ファイルI/O: 独立した CO-RE eBPF で `vfs_read/write` と `vfs_readv/writev` の
  成功した実バイト数を観測。pread/pwrite・位置指定ベクトルI/Oとページキャッシュ経由も含みます。
  ネストしたVFS呼び出しは外側だけ計上し、EOF・失敗した操作は加算しません。
  ファイル名は操作中に取得するため、直後にclose/unlinkされても表示できます。
  パスは呼び出し元のmount namespaceにおける名前で、取得不能ならdevice/inodeの識別子を表示します。
  mmap・io_uring・splice/sendfile・カーネル/IOワーカー・物理ディスク転送量は対象外です。
  プロセス下側の板がファイルを表し、READはプロセスへ、WRITEはファイルへ白い丸が流れます。
  選択するとパスと観測したREAD/WRITE別のバイト数・回数を表示します。
  最終アクセスから30秒間、各プロセス32個・全体512個まで表示し、古い目印から除外します。
  ファイル監視の起動失敗はCPU/IPC・メモリ監視に影響しません。
- メモリ: AMD `ibs_op` を選択プロセスの各スレッドに `perf_event_open` で取得。
  ユーザー空間のサンプルのデータアドレスを4KiBページに集計します。
  低負荷/標準/高密度は初期周期1,000,000/250,000/100,000カウント。
  perf 欠落発生時は周期を最大1,000,000まで増やし、実効周期を表示します。
  IPをデータアドレスとして使わず、無効・未解決アドレスは発光しません。
  MMAP2/exec通知後はマップ再取得まで発光を抑制します。
  サンプルのない場所にアクセスがなかったとは判断できません。

構造は約5秒、CPU/RSSは約1秒、CPU・IPC・ファイルI/O・メモリの活動集計は最大10Hzで更新します。
全体FD走査は1回に100,000FD/4秒、各PIDは4,096マッピング、接続図は約20,000接続が上限です。
時間切れ・権限不足・キュー欠落・未解決は画面に表示します。
構造走査は非停止なので、スナップショット時点が完全に一致するわけではありません。

観測は3D画面の閲覧中のみ動きます。最後の閲覧セッション終了後に停止します。
セッションは10秒更新/30秒期限で、複数閲覧者がいると最も高い要求密度を共有します。
通常のプロセス詳細の選択・ptraceスナップショットとは独立しています。

ビルドには clang（BPF backend）、bpftool、実行カーネルの BTF、libelf 開発ファイル、Cコンパイラが必要です。
Three.js 0.180.0 は同梱で、実行時に外部 CDN へ接続しません。
現在の Linux 7.0 / Ryzen 9 5950X を主対象とします。BTF関数やIBSが非対応の場合は
構造のみ表示し、取得できないセンサーの理由を表示します。

サービス権限を追加する設定は `deploy/procinsh-capabilities.conf` です。
既存の `CAP_SYS_PTRACE CAP_DAC_READ_SEARCH` に `CAP_BPF CAP_PERFMON` を追加します。

```sh
sudo install -m 0644 deploy/procinsh-capabilities.conf /run/systemd/system/procinsh-lan.service.d/capabilities.conf
sudo systemctl daemon-reload
sudo systemctl restart procinsh-lan.service
```

この `/run` の設定はOS再起動まで有効です。

API: `GET /api/space/status`, `GET /api/space/snapshot`,
`POST /api/space/leases` (`{density:1|2|3, token?:string}`),
`DELETE /api/space/leases` (`{token:string}`),
`GET /api/space/events?token=TOKEN` (SSE)。
activity の `files` 配列は `{process_id, resource, path, write, bytes, count}`。
`path` は取得不能ならnull、`resource` はdevice/inode/generationを含む識別子です。
`status.files` は監視状態、`files_lost` は収集開始からの欠落数、`files_coverage` は監視範囲です。
SSE が間に合わない場合は `gap` と現在の構造を返し、古い活動の再生はしません。

検証:

```sh
node tests/space-model.mjs
node tests/space-browser.mjs
# センサー権限を付けたサービスに対して実行。権限不足を成功扱いにしません。
python3 tests/space-live.py http://127.0.0.1:8080
# ファイルI/Oのみの実機検証。IBS不要。URL省略時は一時サーバーを起動・終了します。
python3 tests/space-files-live.py http://127.0.0.1:8080
```

参考: [AMD IBS](https://man7.org/linux/man-pages/man1/perf-amd-ibs.1.html)、
[BPF ring buffer](https://docs.kernel.org/bpf/ringbuf.html)。
