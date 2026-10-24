# ProcInSh の観測処理と権限に関するセキュリティ分析

分析日: 2026-10-24（JST）

著者: OpenAI Codex（AI による分析・執筆）

モデル・推論設定: 実行時の詳細設定は未確認

対象: `e3f6431b5db1f0c1718dad0d789e82f9e6381c25`（診断開始時のローカル HEAD、Cargo バージョン 0.1.20）。リモート main や稼働中バイナリとの一致は確認していない。

## 結論

**HTTP と特権観測は別プロセスになっているが、観測対象への停止の影響、観測サービス全体の可用性、認証なしでの機密情報公開が主要なリスクである。** 「read-only」は対象メモリへの書き戻し API がないという意味では成立するが、対象の実行やホストの負荷に影響しないという意味では成立しない。

HTTP 本体は capability を落とし、専用 socketpair から限定された操作を helper に要求する。任意パス・任意メモリアドレス・任意 syscall を IPC で指定する仕組みはなく、helper は PID と開始時刻を独立に検証する。ELF の同一 FD による読み取り、展開サイズ制限、procfs の読み取り上限も実装されている。これらは有効な防御である。

ただし、helper は広い権限を保ったまま ptrace、perf、BPF、ELF/DWARF 解析を行う。ptrace の停止待ちに期限はなく、同期的な初回観測が唯一の IPC 処理を塞ぎうる。クライアントの30秒タイムアウトは共有接続を閉じるため、ひとつの遅い要求が全利用者の観測を停止させる経路になる。メモリやレジスタへの直接書き込み、任意コード実行、カーネル破壊を実証したわけではない。

## 調査の範囲と確度

HTTP の起動・権限・middleware、IPC schema/transport/client/helper、process monitoring、ptrace/perf、メモリ読み取り、ELF/DWARF/unwind、4種類の BPF C、system monitoring、関連テスト、開発・プレビュー起動スクリプトを静的に確認した。Web の DOM 生成と危険な HTML 挿入 API の検索も行ったが、UI 全体を網羅するブラウザ診断ではない。Linux の仕様は本文中の一次資料と照合した。

「確認済み」はコード上の構造・条件を確認したことを意味する。「要再現」は実際の停止時間、資源消費、攻撃成立を今回の実機試験では確認していないという意味である。優先度は対策・検証の順番であり、CVSS や実証済み脆弱性の重大度ではない。

稼働ホストの sudoers、所有権、ACL、capability、Tailscale ACL、カーネルの修正状況は監査していない。依存関係の最新 advisory との網羅的照合、ファジング、侵入試験、負荷試験は実施していないため、既知脆弱性がないとの判定は含まない。テストの実行状況は末尾に記す。

## 優先度一覧

| 優先度 | 懸念 | 実装の確認 / 影響の確度 | 主な影響 |
|---|---|---|---|
| 高 | 認証なしで観測と環境変数等の取得を許す | 確認済み、到達範囲は配備依存 | 機密漏えい、他者による観測開始 |
| 高 | ptrace の停止待ち・停止中の読み取りに期限がない | 確認済み、長時間停止は要再現 | 対象の停止、タイムアウト |
| 高 | group-stop と割り込み停止を区別しない | 確認済み、停止状態の変化は要再現 | 意図しない再開など |
| 高 | 遅い要求が単一 IPC を塞ぎ、timeout が接続全体を閉じる | 経路は確認済み、実負荷での発生は要再現 | 全クライアントの観測停止 |
| 高 | セッション数と実稼働 collector・TID・perf 資源の総量が一致しない | 確認済み、蓄積量は要再現 | FD・メモリ・CPU の枯渇 |
| 中〜高 | ELF キャッシュと unwinder の保持量に共有バイト予算がない | 確認済み、OOM は未実証 | 特権 helper のメモリ圧迫 |
| 中〜高 | helper・起動アカウント・setcap 配備経路に広い信頼が必要 | 確認済み、侵害は未実証 | 侵害時の影響拡大 |
| 中 | 全システム BPF フックと CPU 数依存の map | 確認済み、負荷量は未測定 | ホストの遅延、メモリ消費、観測欠落 |
| 中 | 数値 PID/TID の検査と実操作の間の競合 | 確認済み、誤対象操作は要再現 | 別対象の一時停止・観測 |
| 残存リスク | カーネル・FFI・依存解析器の欠陥 | 具体的な侵害は未確認 | helper またはホスト全体への影響 |

## 1. 対象データを直接変更する経路

[memory.rs](../../src/privileged_helper/process/memory.rs) は `process_vm_readv` を使い、1回のサイズを65,536 bytesまでに制限し、アドレス加算を検査する。調査した本番コードに `process_vm_writev`、`PTRACE_POKEDATA`、`PTRACE_SETREGSET` はない。[ptrace.rs](../../src/privileged_helper/process/snapshot/ptrace.rs) の要求は SEIZE、INTERRUPT、GETREGSET、DETACH で、レジスタ取得先は helper 自身のバッファである。

[unwind.rs](../../src/privileged_helper/process/snapshot/unwind.rs) は取得済みスタックの範囲を検査し、最大256フレームで打ち切る。命令の逆アセンブルは読み取った bytes の解析であり、対象内でその命令を実行する処理ではない。perf の writable mmap は共有 ring の消費位置を更新するためで、対象のメモリを writable に map するものではない。

4種類の BPF は自身の map とイベント領域を更新する。signal センサーも `signal_generate` の通知を観測するだけで、シグナルを送信しない。対象への書き込み helper や戻り値の強制変更は見つからなかった。ただし読み取りに伴うページ取得や、一時停止と負荷による間接的なデータ整合性への影響は別問題である。

## 2. ptrace の停止と停止状態

### 2.1 停止時間に上限がない

[capture.rs](../../src/privileged_helper/process/snapshot/capture.rs) は初回、および前回完了から10秒後以降に TID ごとに `SEIZE → INTERRUPT → waitpid → GETREGSET → stack read → DETACH` を行う。スタックは最大16 KiBで、ELF 解析は capture から戻って detach した後に行われる。

`PtraceGuard::wait` は `waitpid(..., __WALL)` のブロッキング呼び出しであり、deadline、WNOHANG、キャンセル確認がない。停止中の `process_vm_readv` にも時間制限がない。対象が停止可能になるまでの待機やページ取得が遅延すると、観測側も進めない。ロックを保有する対象スレッドを停止すれば、他のスレッド・プロセスも待たされうる。

`Drop` は通常のエラー時にも detach を試み、EINTR を再試行するが、停止未確認なら `INTERRUPT → wait` を再実行する。Drop 自体も待機しうるため、RAII は停止時間の保証ではない。detach のその他の失敗はログのみである。`PTRACE_O_EXITKILL` は設定しないが、helper 終了時の対象状態は悪条件の実機検証が必要である。

**対策:** perf のみのモード、停止時間の計測、停止待ちの期限、キャンセルと detach の回復設計を優先する。ブロッキング syscall は Rust の timeout だけで中断できるとは限らず、停止操作を担う worker の分離と対象状態の確認を含める。

### 2.2 group-stop の意味を保存することを確認できない

wait の結果は `status >> 16 == 0` のときだけシグナルを保持し、それ以外は signal=0 で detach する。SEIZE による group-stop も `PTRACE_EVENT_STOP` となるため、この条件では通常の割り込み停止と区別できない。停止イベントと WSTOPSIG を合わせて扱う必要がある。[ptrace(2): Group-stop](https://man7.org/linux/man-pages/man2/ptrace.2.html)

既に SIGSTOP で停止している対象、SIGSTOP/SIGCONT と capture が競合する対象について、意図しない再開や停止状態の変化は要再現である。コード内の通常 capture・エラー時 detach テストだけではこの性質を保証できない。

**対策:** stop 種別を保持し、SIGSTOP、SIGTSTP、SIGCONT、通常シグナル、exec、終了の組合せで状態と `TracerPid` を検証する。SIGSTOP の一律再注入は別の停止を生むため、無条件の対策にしない。

## 3. 単一 IPC の停止が観測サービス全体へ波及する

[helper_service.rs](../../src/privileged_helper/helper_service.rs) の dispatch は直列である。通常 API の同時取得をひとつに制限できる一方、`ObserveProcess` は [ProcessMonitor::observe](../../src/privileged_helper/process/monitoring/service.rs) 内で maps 取得と `sampler.bootstrap` を同期実行してから応答する。ELF の I/O や初回 ptrace が遅い間、他の Process、Poll、Close も進まない。

[client.rs](../../src/http_server/client.rs) は64件の queue と単一 I/O worker を使い、各 request は enqueue 後から30秒で timeout する。timeout すると共有 socket を `Shutdown::Both` で閉じる。queue 待ちも期限に含まれるため、ひとつの処理が30秒を超えなくても、待ちの累積によって接続を閉じうる。

接続切断を検知した [helper.rs](../../src/privileged_helper/helper.rs) は停止処理へ進む。collector join の待機は5秒、親の HelperGuard による終了待機は10秒に制限されている。ただしこれは**対象の安全な detach 完了の保証ではない**。HTTP プロセスが動き続けても MonitorClient は再接続せず、supervisor に helper 再起動のループもないため、観測復旧にはアプリケーションの再起動が必要になる。

**対策:** 重い初期化を直列 dispatcher から分離し、受付・実行・取消の期限を分ける。IPC フレームの対応関係を壊さず、個別処理の失敗を局所化する。helper の異常終了と復旧を明示的に管理し、遅い初期観測と同時 Poll の試験を追加する。

## 4. セッション上限と実際の資源使用量

process 観測は32セッションまでだが、同一 PID の collector は共有されず、接続ごとに Sampler と ElfCache を持つ。各 TID は最大2本の perf event と各17 pagesの mapping を使う。4 KiB pageなら約136 KiB/TID、1,000 TID × 32セッションで約4.15 GiB、64,000 FD相当となる。全 open が成功した場合の概算であり、実際は OS の FD・perf メモリ制限等で先に失敗する場合がある。[Linux kernel: Perf events and tool security](https://docs.kernel.org/admin-guide/perf-security.html)

CPU-clock は約100 Hzだが、context-switch event は period=1であり、その負荷は100 Hzに固定されない。アプリケーションに全 TID 数・全 FD 数・全 mapping 容量の共有予算はない。

さらに ObservationPermit の Drop はキャンセルフラグを立て、直ちに viewers を減らす。worker が `sampler.poll` 内の ptrace や I/O から戻ることは待たない。32という上限は登録中セッションの上限であって、停止できていない worker の上限ではない。**定期観測中の worker が詰まったまま Close と再接続を繰り返す条件では、未終了 worker が32を超えて残る経路がある。** 初回 bootstrap が詰まる場合は第3節の共有 IPC 停止が先に問題になる。蓄積の実機再現は未実施である。

15秒の lease は未 polling セッションを回収するが、実行中 syscall を中断しない。また expire は dispatch と serve のループで実行されるため、同期要求が詰まっている間に期限どおり回収する保証はない。

**対策:** 実 worker の終了まで資源 permit を保持し、PID ごとの共有 collector、全 TID/FD/bytes 予算、再接続頻度制限を設ける。キャンセル済み worker と稼働中 worker を別々に数え、停止しない worker を含めて受付を制限する。

## 5. ELF/DWARF と procfs の防御と残る制限

[symbol/cache.rs](../../src/privileged_helper/process/snapshot/symbol/cache.rs) は O_PATH で inode を固定し、regular file と device/inode を確認した後に `/proc/self/fd/...` から開く。同じ descriptor から最大128 MiB+1を読み、前後の identity を検査する。pathname を確認後にそのまま開き直す設計ではなく、FIFO・device の読み取りも拒否する。

ELF は展開前に1 section 32 MiB、全 section 64 MiBを検査し、圧縮方式を無圧縮と zlib に限定する。symbol 数262,144、名前総量8 MiB、DWARF unit 数16,384にも上限がある。これらは入力由来の無制限割当てを減らす。ただし metadata の一致は、同時更新されたファイルの内容が完全に一貫する保証ではない。

cache は64件・128 MiBを目安に clear するが、バイト計算は `entries.keys().map(|key| key.2)`、つまり**元ファイルのサイズ**である。展開済み section、symbol、addr2line の解析結果、allocator overhead の実保持量ではない。小さい圧縮 ELF が大きい section に展開される場合、この差は大きい。

さらに [StackUnwinder::refresh](../../src/privileged_helper/process/snapshot/unwind.rs) は全 executable mapping を巡回し、`elf.unwind.clone()` を module として保持する。Arc の共有データは cache の clear 後も module 側から参照されうる。したがって64件という cache 件数も、unwinder が保持する全 module 数や bytes の上限にはならない。圧縮 ELF を多数 map する対象などで、大きな保持量となる可能性がある。OOM は未実証である。

[procfs.rs](../../src/privileged_helper/process/procfs.rs) は text を16 MiBに制限し、maps/smaps もこの経路を使う。environment は1 MiB、auxv は64 KiBまでである。一方、これらは入力単位の上限であり、全解析結果・セッション・履歴・JSON Value の合計 RSS 上限ではない。IPC の32 MiB response 上限も、`to_value` や snapshot 作成の**後**の framing に適用される。

**対策:** cache と unwinder をまたぐ保持 bytes/module 数の予算を導入し、展開後の容量を計上する。procfs・ELF の遅い filesystem I/O と解析時間を制限できる worker 設計を検討する。ELF/DWARF 解析を低権限プロセスへ移すと、解析器欠陥の影響も小さくできる。

## 6. 権限分離の保証範囲と配備の信頼境界

### 6.1 HTTP 本体と helper

[launch.rs](../../src/http_server/launch.rs) は runtime の生成前に継承 FD を閉じ、HTTP 本体の effective/permitted/inheritable/ambient capability を消す。dumpable を無効にし、helper の exec 後に `no_new_privs` を設定する。helper 起動は環境変数を消去し、必要な RUST_LOG と private endpoint を渡す。

helper は SO_PEERCRED の PID/UID と実親 PID を検証する。通常の他プロセスが接続できる公開 Unix socket はない。helper 側も dumpable を無効にし、権限を `CAP_SYS_PTRACE`、`CAP_BPF`、`CAP_PERFMON`、`CAP_DAC_READ_SEARCH`、`CAP_SYS_ADMIN` の集合内に絞る。CAP_SETPCAP がある起動経路では NOROOT と bounding set の制限も行う。

この分離により、HTTP 内の欠陥から直接 ptrace や BPF の FD を利用する経路は狭まる。一方、侵害された HTTP は既存 IPC から全ての許可済み観測操作を要求できる。helper の PID/開始時刻検証は対象の同一性検証であり、利用者ごとのアクセス制御や対象 allowlist ではない。

また UID/GID・補助グループ・ファイルシステムは起動者のものを保ち、アプリ独自の seccomp や filesystem sandbox は設定しない。capability=0 は別 OS ユーザーへの移行を意味しない。root 起動を含め、起動アカウントが所有するファイルや同一 UID の資源への通常のアクセスは別に考える必要がある。

helper 内の解析器が侵害されれば、SYS_PTRACE は書き込みにも利用可能であり、SYS_ADMIN も network namespace だけに限定されない。DAC_READ_SEARCH はファイルの読取・探索制限を広く迂回する。これらの意味は capability の仕様に基づくもので、今回その侵害経路を見つけたわけではない。[capabilities(7)](https://man7.org/linux/man-pages/man7/capabilities.7.html)

**対策:** 非 root の専用実行アカウント、対象 allowlist、低権限解析 worker、用途ごとの helper 分割を検討する。起動者のファイルへの権限も含めてサービス隔離を設計する。

### 6.2 file capability と更新経路

[dev_run.sh](../../scripts/dev_run.sh) と [プレビュー手順](PREVIEW.md) は helper にだけ file capability を設定する。main/release の配備スクリプトは mode 0755の helper candidate に setcap し、同一 filesystem 内でペア用ディレクトリへ移動する。

文書の sudoers 例はユーザーが書ける固定 candidate パスへの setcap を許す。ファイル内容の検証は行わないため、ビルドアカウントとその入力を信頼する必要がある。helper の親確認も、同じ条件で socketpair を作る任意のランチャーを識別するものではない。実行可能な他ユーザーの範囲は祖先ディレクトリのアクセス権にも依存する。

[validate_helper_path](../../src/http_server/launch.rs) は絶対パスと regular file を確認するが、owner/mode、署名、ハッシュは検証しない。これは配備経路を信頼する設計である。file capability はサービス終了だけで消えず、旧ペアに残る可能性もある。稼働ホストに危険な設定が実在するかは未確認である。

**対策:** 実行者・ビルドアカウント・更新入力を信頼範囲として明記し、必要なら root 管理の成果物検証と配備経路を用いる。旧 capability 付き成果物と sudoers を含めてアクセス権を監査する。

## 7. BPF のホスト負荷と数値 ID の競合

[system/service.rs](../../src/privileged_helper/system/service.rs) は system 観測を共有し、viewer がなくなると activity collector を解放する。system の32購読ごとに BPF map 一式を増やす構造ではない。一方、購読中のフックは全ホストの sched switch、file I/O、pipe/socket I/O、signal に及ぶ。

[sched.bpf.c](../../src/privileged_helper/system/sched.bpf.c) の LRU_PERCPU_HASH は65,536件で、値16 bytesを possible CPU ごとに持つ。値領域だけでも概算 `65,536 × 16 × possible CPU数`、64 CPUなら64 MiBとなり、管理構造は別である。[files.bpf.c](../../src/privileged_helper/system/files.bpf.c) は4096件の pending mapに4096 bytesの path等を保持する。file/IPC ringは各8 MiB、[signals.bpf.c](../../src/privileged_helper/system/signals.bpf.c) は256 KiBである。map や ring が満杯のときの欠落集計はあるが、イベント頻度そのものを制限するものではない。

kernel verifier はプログラムの検査を行うが、フック頻度に伴う負荷や kernel/JIT/helper の欠陥まで本診断で排除できない。ホストを破壊する具体的な BPF バグは確認していない。

PID/開始時刻は helper と取得処理で繰り返し検証され、TID の開始時刻も perf 管理に使われる。ただし最終的な ptrace/perf 呼び出しは数値 TID であり、検査と操作の間の終了・再利用を原子的に防ぐものではない。事後の PID 検査は誤ったレスポンスの抑止には役立つが、既に発生した停止を取り消せない。

**対策:** CPU 数とイベント量に応じた map/負荷予算、センサー単位の無効化、重要な workload の遅延測定を行う。短命スレッド、exec、PID 再利用と観測が競合する場合も使い捨て環境で検証する。

## 8. HTTP 公開と機密情報

[main.rs](../../src/main.rs) の既定待受は127.0.0.1で、非 loopback は `--allow-non-loopback` が必要である。[middleware.rs](../../src/http_server/middleware.rs) は Host、Origin、Sec-Fetch-Site を検査し、CSP、no-store、nosniff を付ける。Web の共通 DOM helper は textContent を使用し、今回検索した `web/src` に innerHTML、insertAdjacentHTML、eval の直接使用は見つからなかった。これは XSS 全般の不在を証明するものではない。

アプリケーション認証と TLS はない。Origin や Fetch Metadata を省略し正しい Host を送れる通常の HTTP クライアントに対し、これらの middleware は利用者認証として機能しない。loopback でもローカルの別ユーザーからの利用を区別しない。環境変数、cmdline、FD、maps、レジスタ等は機密情報になりうる。任意アドレスの raw memory read API がないことだけで、漏えいを防げるわけではない。

[preview_bind.sh](scripts/preview_bind.sh) は Tailscale IPv4、127.0.0.1、::1で待ち受ける。Tailscale の経路・ACL は別の防御だが、到達可能な利用者をアプリ内部でさらに区別する機能はない。詳細 SSE の開始は観測だけでなく ptrace/perf の起動にもつながる。

**対策:** 利用者と対象を信頼できる範囲に限定する。リモート公開には到達制限と認証を設け、リバースプロキシを使うなら裏側ポートへの直接到達を防ぐ。保存・共有する観測データも、実際に含まれるプロセス情報に応じて機密性を管理する。

## 9. 対策と追加検証の順番

1. **公開範囲と対象を制限する。** 接続者・観測対象・起動アカウント・配備入力の信頼境界を確認する。
2. **ptrace による停止を選択可能にする。** 停止状態の保存、停止時間、detach の回復を fixture で検証する。
3. **ひとつの要求によるサービス全体停止を防ぐ。** 遅い bootstrap、IPC queue の待ち、timeout、helper 終了後の復旧を検証する。
4. **実資源の総量を制限する。** 未終了 worker、TID、perf FD/mmap、ELF 展開量、unwinder module の保持量を合算する。キャンセルと再接続も負荷試験に含める。
5. **特権のある解析面を減らす。** 低権限 worker と専用アカウントを検討し、setcap 配備経路と旧成果物の権限を監査する。
6. **kernel ごとの実機検証と依存監査を行う。** BPF の部分 attach 失敗・終了時解放、CPU 数とイベント量による負荷、依存 advisory を確認する。

### 今回の検証記録

`bash scripts/docker_test.sh --network host ordinary` を実行し、終了コード0を確認した。Rust は135件成功、失敗・ignore・filter は0。Web は24ファイル・163件成功。listen policy、Python テスト（21件と3件）、Vite development integration も成功した。最初の既定 bridge network ではイメージビルド中に Ubuntu リポジトリの DNS 解決に失敗したため、同じスクリプトの host network 指定で実行した。テスト対象や権限不足のスキップは追加していない。

関連する既存テストには、HTTP/helper の各 thread の capability と起動 UID/GID、HTTP 側の FD 分離、IPC の不正フレーム・期限・対象 identity、購読終了と lease、ptrace のエラー時 detach、ELF 入力制限がある。通常テストの成功は、今回指摘した悪条件のすべてを再現したことを意味しない。

`kernel`、`browser`、`checks` suite は今回実行していない。live eBPF の全センサー検証、group-stop の専用再現、長時間停止・再接続蓄積・メモリ枯渇の負荷試験も未実施である。負荷・障害系の追加試験は重要なプロセスのない使い捨て VM で実施する。

変更はレポートのみである。旧レポートは本文の分析日を suffix とした [SECURITY_ANALYSIS_2026-10-03.md](SECURITY_ANALYSIS_2026-10-03.md) に、内容を変更せず保存した。旧レポート内のコードリンクは旧構成のため、当時の対象 commit で参照する。本レポートのローカルリンクの存在と、旧レポートのバイト単位の一致を確認した。
