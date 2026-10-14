# 権限分離と IPC の境界

ProcInSh は一つの実行ファイルから、Tokio runtime を作る前に `fork()` して二つのプロセスを起動します。

```text
CLI・listen 設定の検証
  └─ socketpair + fork
       ├─ 親: MonitorService、/proc、ptrace、perf、BPF、netns、ELF/DWARF、unwind
       └─ 子: capability 破棄、Tokio、Axum、HTTP JSON、SSE、差分生成、逆アセンブル
```

実装の入口は [privsep.rs](../src/http_server/privsep.rs) です。HTTP の listen は子だけで行います。親の domain collector は HTTP のパス・ヘッダー・ステータスを解釈しません。観測履歴、CPU/rate 計算とセンサー集計は collector にあり、子は処理済みの観測値から HTTP の表現を生成します。逆引き DNS は system snapshot collector が行います。

## 起動時の権限

親は起動時に利用できる capability のうち、`CAP_DAC_READ_SEARCH`、`CAP_SYS_PTRACE`、`CAP_SYS_ADMIN`、`CAP_PERFMON`、`CAP_BPF` だけを permitted/effective に残します。inheritable/ambient は空にし、`no_new_privs` と dumpable=0 を設定します。CAP_SETPCAP があれば不要な bounding capability も落とし、UID 0 による exec 時の capability 特例を securebits で無効化します。能力が不足していてもサーバーは起動でき、取得に失敗した機能は既存のエラーや sensor status で通知します。CAP_SYS_PTRACE のない root コンテナでは、対象 root プロセスの permitted set が親の set を超えると、Linux の ptrace capability 検査により保護された `/proc` 情報を取得できないことがあります。

子は permitted/effective/inheritable/ambient をすべて空にし、`no_new_privs` と dumpable=0 を設定してから runtime を作ります。`no_new_privs` は exec を跨いで保持され、setuid や file capability による再昇格を防ぎます。CAP_SETPCAP がある起動では bounding set も空にします。file capability だけで起動して CAP_SETPCAP がない場合は bounding set が残りますが、exec による能力の取得は許可されません。[Linux capabilities](https://man7.org/linux/man-pages/man7/capabilities.7.html)、[PR_SET_NO_NEW_PRIVS](https://man7.org/linux/man-pages/man2/PR_SET_NO_NEW_PRIVS.2const.html)

通常ユーザーの起動では、そのユーザーの real UID/GID を使います。sudo 起動では非 root の `SUDO_UID`/`SUDO_GID` を使い、root から直接起動した場合は UID/GID 65534 を使います。不正・不完全な sudo ID は起動エラーです。root の子は supplementary groups を空にして real/effective/saved UID/GID をすべて変更します。親の real UID は子と一致させるため、CAP_KILL を保持せず終了シグナルを転送できます。

stdio 以外の継承 FD は socketpair の作成前に閉じます。初期 socketpair の各端点だけを fork で継承し、不要な側をすぐ閉じます。ソケットは close-on-exec です。target、ELF、perf、BPF、pidfd、診断ソケットはすべて親が開いて所有します。IPC は通常の read/write だけを使い、`sendmsg` や `SCM_RIGHTS` は使いません。

## IPC と資源制限

[protocol.rs](../src/http_server/privsep/protocol.rs) が要求・応答・イベントの型を定義します。

| 要求 | 親で行う操作 |
|---|---|
| Processes | プロセス一覧の取得 |
| Process | PID・開始時刻と列挙型で指定した observation/threads/maps/environment/auxv/fds の取得 |
| ObserveProcess / ObserveSystem | 独立した継続観測の開始 |
| Poll | 観測セッションから、待たずに次の構造・活動・プロセス観測を取得 |
| Close | 観測セッションを解放 |

パス、任意アドレス、syscall 番号、FD や任意の読み取りサイズを要求に含めることはできません。親は HTTP の検証と独立して PID と開始時刻を検査し、単発取得の完了後も再検査します。数値 PID/TID の検証とカーネル操作の間の競合は完全には排除できません。

| 対象 | 制限 |
|---|---|
| フレーム | 4-byte big-endian length + JSON。要求 1 KiB、応答 32 MiB |
| 子の要求キュー | 64 件。一つの IPC worker が順番に交換 |
| 親の単発操作 | 同時に 1 件 |
| フレームの入出力 | 各 30 秒。途中の分割送受信で期限を延長しない |
| 子の要求全体 | キュー待ちを含め 30 秒。期限切れで接続を閉じる |
| 観測 | process/system それぞれ最大 32 接続、15 秒間 poll がなければ解放 |
| イベント | process は最新状態、system は既存の 16 件の broadcast buffer と gap 通知 |
| メモリ読み取り | 既存の 1 回 64 KiB、stack 16 KiB、命令 256 bytes の制限 |

IPC に HTTP の文字列を透過転送せず、構造化済みの値を返します。親で IPC 用のシリアライズ、子で HTTP 用のシリアライズを行います。system の完全 snapshot を受け取った子が接続ごとの差分と sequence を生成します。命令 bytes の decode も子で行います。応答をシリアライズするバッファはサイズ制限付きです。構築前の domain collection 全体のメモリ使用量を 32 MiB に制限するものではありません。

## ELF/DWARF と unwind のレビュー

ELF/DWARF の解析、シンボル解決、stack unwind は意図的に親へ配置しています。解析対象を読み取るための FD は子へ渡しません。

- 選択元は親が取得した memory mapping です。`/proc/PID/map_files/START-END`、`/proc/PID/exe`、対象 root 内の mapping pathname だけを候補にします。
- O_PATH で対象を固定し、regular file と device/inode の一致を確認してから、その FD を `/proc/self/fd` 経由で読み取り用に開きます。名前の置換で別の inode を解析する競合を防ぎます。
- ファイルは最大 128 MiB まで読み、読み取りの前後で device/inode/size/mtime を照合します。メモリマップした target file は使いません。
- 展開前に各 section の宣言サイズを検査します。1 section 32 MiB、section 全体 64 MiB が上限です。ELF 以外は受け付けません。圧縮は非圧縮/Zlib のみを扱います。object の Zstandard 展開経路は出力・window の割り当てをここで制限できないため、Zstandard を含む ELF は解析しません。
- ELF symbol entry は 262,144 件、名前は 1 件 4 KiB・合計 8 MiB、DWARF unit は 16,384 件までです。source の function/file は各 4,096 文字まで、inline frame は 256 件、stack unwind も 256 frame までです。
- cache は最大 64 ファイル・入力ファイルサイズ合計 128 MiB を目安に全体を evict します。すでに resolver/unwinder が保持する Arc は生存し、解析後の索引・文字列・allocator overhead は入力サイズと同じにはなりません。
- `object 0.36.7` の section decompression、`gimli 0.31.1` の DWARF reader と `addr2line 0.24.2` の Context、`framehop 0.16.0` の unwind が親の parser boundary です。Cargo.lock で固定します。addr2line の filesystem Loader は使わず、所有した section bytes から Context を作ります。DWO/DWP・補助 debug file を自動取得しません。

これらは入力・展開・件数の上限です。解析ライブラリの単一呼び出し内の CPU 時間や割り当て、同じ inode への並行書き換えを完全には制御しません。byte 制限だけで厳密な wall-clock 上限は保証できません。CFI と DWARF の不正データは親を攻撃しうるため、依存 parser の更新時も確認が必要です。seccomp と操作別の capability 分割は設けていません。

## 終了と検証

親は SIGINT/SIGTERM を子へ転送し、collector を停止します。子の異常終了や IPC 切断でもセッションを破棄します。親は通常の IPC 処理とは別に socket の切断を監視し、取得処理が停滞していても終了処理を開始します。子の終了待ちは 10 秒、IPC worker と collector の終了待ちはそれぞれ 5 秒で打ち切り、エラー終了します。プロセス終了時にはカーネルが残る FD を閉じます。ptrace の停止・読み取り自体を中断できる保証とは異なります。

子には credentials 変更後に `PR_SET_PDEATHSIG(SIGTERM)` を設定し、直後に親 PID を検査して起動時の競合も検出します。親が SIGKILL で終了しても HTTP worker を停止します。親は `waitid(WEXITED)` で子を回収し、観測の ptrace stop を終了と取り違えません。回収した PID に再度シグナルを送ることはありません。[PR_SET_PDEATHSIG](https://man7.org/linux/man-pages/man2/PR_SET_PDEATHSIG.2const.html)

単体テストはフレーム検査・期限、PID 検証、セッション解放、切断、capability を持つ実行ファイルの再 exec 後の権限破棄を確認します。HTTP 結合テストは実際の親子の全 thread の permitted/effective と親子の inheritable/ambient、UID/GID、no_new_privs、親子それぞれの異常終了を確認します。

```sh
PROCINSH_REQUIRE_PERF=1 ./scripts/dev_test.sh
cargo build --locked
PROCINSH_REQUIRE_PERF=1 PROCINSH_BINARY=./scripts/dev_run.sh cargo test --locked --test http --test build_git
```

HTTP の認証・認可は提供していません。IPC の検証は、信頼されていない HTTP client に観測を許可するかどうかの認可を代替しません。listen/Host/Origin の制限を守り、非 loopback 公開は管理されたネットワーク内に限定します。
