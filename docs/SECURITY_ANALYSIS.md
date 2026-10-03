# ProcInSh の観測処理と権限に関するセキュリティ分析

分析日: 2026-10-03

対象: `99c0483194459e6c3c3e2e7151e1202d31de18a7`（PR 作成時に取得した最新 main、バージョン 0.1.6）

## 結論

現実的にまず心配すべきなのは、**ptrace による停止・再開が観測対象の実行を変えること**と、**perf/eBPF の収集負荷によってホストや対象プロセスの可用性が下がること**である。「read-only」は対象メモリへの書き込み API がないという意味では成立するが、対象の動作に影響しないという意味では成立しない。

本番コードには、対象メモリ・レジスタ・ファイル内容へ直接書き込む処理や、BPF から対象のユーザーメモリを変更する処理は見つからなかった。通常の実装バグだけで対象のメモリ内容を上書きする経路よりも、一時停止、シグナル処理の変化、資源不足を通じて、対象がタイムアウトしたり異常終了したりする経路のほうが具体的である。対象アプリケーションの設計によっては、その結果として処理中のデータや外部システムとの整合性を損なう可能性がある。

eBPF はカーネル内で実行されるが、この実装の書き込み先は自身の map とイベントバッファであり、VFS の書き込み引数やスケジューラの設定を変更していない。カーネル破壊の具体的な不具合は今回の静的調査では確認していない。ただし、verifier・JIT・helper・tracing/perf 実装自体の不具合まで安全性を保証できるものではなく、負荷によるホスト全体への影響は別途存在する。

## 調査の範囲と確度

Rust の本番コード、3種類の BPF C コード、起動・テスト・常設プレビュー用スクリプト、既存テストを読み、Linux の仕様資料と照合した。既知の非 loopback 公開による情報漏えいは前提とし、停止状態、メモリ取得、カーネルフック、資源管理、権限の境界を中心に調査した。

以下では「確認済み」はコードから確認できる実装上の事実、「要再現」はその実装から導かれるが、発生条件・結果を実機で確かめていない懸念を示す。攻撃の成立やデータ破壊を実証したという意味ではない。今回はサーバーの起動、権限付きテスト、負荷試験、カーネル障害の再現を行っていない。稼働ホストのカーネル版・修正状況・sudoers・ファイル権限も監査対象に含めていない。

## 優先度一覧

優先度は対策・追加検証の順番であり、CVSS や実証済み脆弱性の重大度ではない。

| 優先度 | 懸念 | 実装の確認 / 影響の確度 | 主な影響 |
|---|---|---|---|
| 高 | ptrace 待機・停止中の読み取りに期限がない | 期限なしは確認済み、長時間停止は要再現 | 対象停止の長期化、観測・終了処理の停滞 |
| 高 | group-stop と割り込み停止を区別しない | 区別しない実装は確認済み、停止状態の変化は要再現 | SIGSTOP 等で停止した対象の意図しない再開など |
| 高 | 接続ごと・全 TID に perf と ptrace を設定する | 構造は確認済み、負荷量は未測定 | FD・メモリ・CPU の消費、対象のタイムアウト |
| 高 | HTTP と解析処理が強い capability を持つ同じプロセスで動く | 確認済み、侵害経路は未実証 | 侵害時に他プロセスの変更へ拡大 |
| 中〜高 | 全システムの BPF フックと大きな map | サイズ・フックは確認済み、影響量は未測定 | ホストの遅延・メモリ圧迫、観測欠落 |
| 中 | PID/TID 検証と実操作の間の競合 | 数値 ID を使う構造は確認済み、誤対象操作は要再現 | 別のスレッドへの一時停止・観測 |
| 中 | ELF/DWARF の取得・展開の資源制限とファイル検証に隙間 | 確認済み、DoS・侵害は未実証 | 観測側のメモリ消費・I/O 停滞、依存ライブラリへの攻撃面 |
| 残存リスク | カーネルの BPF/perf/ptrace 実装の不具合 | procinsh 起因の破壊は未確認 | 条件次第でホスト全体の障害 |

## 1. 対象のデータを直接書き換える経路

[memory.rs](../src/http_server/process/memory.rs) の `read_raw` は `process_vm_readv` を使い、1回の読み取りを最大 65,536 bytes に制限し、アドレス加算の overflow を検査する。ローカルの出力バッファとリモートの入力アドレスは正しい方向に渡されている。`process_vm_writev` は使っていない。これは他プロセスから呼び出し側へデータを転送する API である。[process_vm_readv(2)](https://man7.org/linux/man-pages/man2/process_vm_readv.2.html)

[ptrace.rs](../src/http_server/process/snapshot/ptrace.rs) の本番コードが使う要求は `PTRACE_SEIZE`、`PTRACE_INTERRUPT`、`PTRACE_GETREGSET`、`PTRACE_DETACH` である。`PTRACE_POKEDATA`、`PTRACE_SETREGSET`、命令の注入、ブレークポイントの書き込みはない。`NT_PRSTATUS` の取得先は procinsh 自身の `user_regs_struct` で、返されたサイズも検査する。対象へのシグナル送信 API は HTTP router に存在せず、テスト fixture 用の `kill` は本番処理とは別である。

スタックは ptrace/perf で取得したコピーを使い、[unwind.rs](../src/http_server/process/snapshot/unwind.rs) はそのスライスの範囲内を読む。最大256フレームの上限もある。[disasm.rs](../src/http_server/process/snapshot/disasm.rs) は命令を最大256 bytes 読んで解析するが、その命令を実行しない。

したがって、壊れたスタックや不正な RIP/RSP を観測しただけで、それを対象へ書き戻す仕組みはない。ただし、読み取りでもページ取得や I/O、キャッシュ状態などに影響しうるため、「書き込みがない」と「実行への影響がない」は別である。

## 2. ptrace: 一時停止と停止状態の扱い

### 2.1 短時間停止は必ず発生し、停止時間の上限はない

[capture.rs](../src/http_server/process/snapshot/capture.rs) の `bootstrap` と `capture_ptrace` は観測対象の各 TID を順に捕捉する。初回と、完了から10秒後以降の定期観測で実行される。各 TID は `SEIZE → INTERRUPT → waitpid → GETREGSET → スタック読み取り → DETACH` の間停止する。

ロックを持つスレッドを停止すると、他スレッドや別プロセスもそのロック待ちになりうる。リアルタイム処理、heartbeat、短い deadline を持つ通信やトランザクションには、メモリを変更しなくても影響する。全スレッドを同時停止していないため、共有メモリや maps を含む観測がプロセス全体の一貫した snapshot になるわけでもない。

`Attachment::wait` は `waitpid(tid, ..., __WALL)` をブロッキングで呼ぶ。期限、キャンセルの確認、`WNOHANG` による監視がない。対象がカーネル内の長い待ちから停止可能な状態へ戻らない場合、観測側は待ち続けうる。また、停止後の `process_vm_readv` にも時間の上限がない。ページ取得などで読み取りが遅れる場合、16 KiB というサイズ制限だけでは停止時間を制限できない。

この待機中は [monitoring/service.rs](../src/http_server/process/monitoring/service.rs) のキャンセル・停止フラグを確認できない。接続が切れても `spawn_blocking` 内の初期化を直ちに止める仕組みはなく、collector の `join` にも期限がない。停止時間を含む悪条件での再現確認が必要である。

**対策:** ptrace を既定では使わない perf 中心のモードを設け、停止が必要な場合は明示的に選択する。停止待ちの deadline と停止時間の計測を導入し、停止中の読み取りを縮小する。単にタイムアウトで関数を抜けるだけでは不十分で、以下の detach まで含めて回復方法を設計する必要がある。読み取り中のブロッキング syscall は Rust 側の timeout だけで中断できるとは限らないため、観測 worker のプロセス分離も検討する。

### 2.2 group-stop を「合成停止」とまとめて扱っている

`Attachment::wait` は `status >> 16 == 0` の場合だけ `WSTOPSIG` を保存し、それ以外では `signal = 0` のまま detach する。コメントは `PTRACE_EVENT_STOP` を合成停止として説明しているが、`PTRACE_SEIZE` では SIGSTOP/SIGTSTP 等による **group-stop も `PTRACE_EVENT_STOP` として通知される**。`WSTOPSIG` とイベント種別を合わせた分類が必要であり、再開操作は停止状態を変えうる。[ptrace(2): Group-stop / PTRACE_DETACH](https://man7.org/linux/man-pages/man2/ptrace.2.html)

現実装は通常の割り込み停止と group-stop を区別しないため、既に停止している対象や、捕捉と SIGSTOP/SIGCONT が競合した対象で、停止・再開の意味を保存できるかが懸念となる。実機で意図しない再開が起きることを確認したわけではないが、ユーザーが気にしている「観測だけで対象の挙動を変える」経路として優先的に検証すべきである。

**対策:** stop の種類を明示的に保持し、group-stop を保存したまま観測・detach する設計を検証する。`PTRACE_LISTEN` は停止状態を扱うための仕組みだが、これを追加するだけで detach の問題が解決するとは限らない。SIGSTOP を一律再注入する方法も、新しいシグナルやプロセス全体の停止を生むため、無条件には使えない。

### 2.3 エラー時の detach は配慮されているが、回復を保証しない

`SEIZE` 成功後すぐに `Attachment` を作るため、通常の早期 return と panic unwind では `Drop` が detach を試みる。`waitpid` と `DETACH` の `EINTR` は再試行し、通常の signal-delivery-stop は保存したシグナルを detach 時に渡す。この配慮は有効である。

一方、停止していない場合の `Drop` 自体も `INTERRUPT → wait` を行い、そこでブロックしうる。detach が `EINTR` 以外で失敗すると警告だけを出して終了し、attachment の回復を追跡しない。対象終了による `ESRCH` は問題のない場合もあるので、すべての失敗を対象の取り残しと断定してはいけない。しかし、生存中の対象について回復できたことの確認もない。

`PTRACE_O_EXITKILL` は設定していないため、観測側の終了で対象を意図的に殺す設定ではない。tracer 終了時には通常はカーネルが detach を処理するが、group-stop は別扱いである。[ptrace(2): Attaching and detaching](https://man7.org/linux/man-pages/man2/ptrace.2.html) 観測側が生きたまま待機するケースを、この終了時処理だけで解決できるわけではない。

**対策:** 生存・終了・exec/TID 変化と detach エラーを分けて記録し、回復失敗した観測を正常として継続しない。回復目的で対象を `SIGKILL` する設計はデータ損失につながるため避ける。

### 2.4 シグナルや exec との競合は追加検証が必要

通常シグナルの再注入は実装されているが、捕捉中の SIGCONT、停止シグナル、スレッド終了、非 leader スレッドからの exec との競合を網羅したテストは確認できなかった。`PTRACE_SEIZE` を採用しているため、`PTRACE_ATTACH` が送る SIGSTOP に固有の問題を、そのままこの実装の不具合として扱うべきではない。[ptrace(2): PTRACE_SEIZE](https://man7.org/linux/man-pages/man2/ptrace.2.html)

既存の [ptrace.rs のテスト](../src/http_server/process/snapshot/ptrace.rs) は取得失敗時の detach と既に traced な対象を、[capture.rs の live tests](../src/http_server/process/snapshot/capture.rs) は定期捕捉や perf との切り替え等を検査する。これらは有用だが、停止状態やシグナル意味の保存まで保証しない。

## 3. PID/TID の再利用と snapshot の競合

[identity.rs](../src/http_server/process/identity.rs) は PID と `start_time_ticks` を検査し、`Sampler::poll` もスレッドの開始時刻が変われば古い perf event を破棄する。誤対象の結果を返すリスクを減らす実装である。

ただし、検査と `ptrace(tid)` / `perf_event_open(tid)` / `process_vm_readv(pid)` は別操作である。`capture_ptrace` は保持している TID に対して捕捉を始め、全 TID の処理後にプロセス識別子を再検査する。直前にその TID が終了・再利用されると、別の TID を一時停止する可能性を数値 ID と事後検査だけでは除外できない。開始時刻は tick 単位であり、exec によるアドレス空間変更も、同一 PID・開始時刻のまま起きうる。

**対策:** 操作直前の TID 所属・開始時刻検証と、捕捉後の対象確認を追加する。これは競合の窓を狭める対策であり、原子的な識別の代用にはならない。pidfd は生存監視に役立つが、この実装の ptrace/perf の数値 TID 引数を単純に pidfd に置き換えられるわけではない。停止操作を減らすことも、誤対象操作の影響を小さくする。

## 4. eBPF: カーネル内の書き込み先と残る影響

### 4.1 観測対象や VFS のデータを直接変更する BPF ではない

| BPF | 接続点 | 読むもの | 書き込み先 |
|---|---|---|---|
| [sched.bpf.c](../src/http_server/system/sched.bpf.c) | `tp_btf/sched_switch`、`sched_process_exit` | task の識別・開始時刻、時刻 | CPU ごとの現在状態と集計 map |
| [ipc.bpf.c](../src/http_server/system/ipc.bpf.c) | pipe の `fexit`、socket の `tp_btf` | file/inode の識別、元処理の返り値 | ring buffer、欠落数 map |
| [files.bpf.c](../src/http_server/system/files.bpf.c) | VFS の `fentry/fexit`、`security_file_permission` の `fentry`、終了 tracepoint | file/inode/path、元処理の返り値 | pending map、ring buffer、欠落数 map |

`bpf_probe_write_user`、`bpf_override_return`、シグナル送信、LSM の許可・拒否判定、XDP/TC のパケット変更は使っていない。`security_file_permission` に接続しているのも LSM policy の置換ではなく、path を取得する tracing hook である。BPF 関数の `return 0` を VFS の元の返り値を書き換える処理と解釈してはいけない。

verifier はアクセス範囲、ポインタ種別、helper 引数などを検査する。そのため、通常の BPF C の不正なポインタ操作は、ロード拒否になるべきもので、ネイティブなカーネルモジュールの任意書き込みとは異なる。ただし verifier が正常に動作することを前提とする。[Linux kernel: eBPF verifier](https://docs.kernel.org/bpf/verifier.html)

### 4.2 全システムの hot path に追加処理が入る

PID allowlist や cgroup によるフック内フィルタはない。システム観測が有効な間、scheduler、pipe/socket、対象 VFS 処理のたびに map 操作やイベント生成が走る。ファイルイベントは成功した I/O ごとに **4,144 bytes** を ring buffer にコピーする。短い I/O が非常に多い workload では、実データ量に対して観測処理の比率が大きくなりうる。path の解決や map が満杯の場合の欠落処理にもコストがある。

map / buffer は上限付きだが、小さいとは限らない。

- IPC と file の ring buffer はそれぞれ 8 MiB、合計 16 MiB。
- file の pending map は最大4,096件。構造体サイズから値部分だけで約16 MiB規模になり、管理領域は別途必要。
- scheduler は最大65,536件の `LRU_PERCPU_HASH`。値16 bytes × 65,536 × possible CPU 数だけで、CPU 1個あたり約1 MiB、256個なら約256 MiBとなる概算。これは map 管理領域等を含まない設計上の計算で、実測値ではない。
- [sched.rs](../src/http_server/system/sched.rs) は約100 msごとの集計で最大65,536 keyを列挙し、CPU ごとの値を取得する。CPU 数と登録数の増加は user space の収集負荷も増やす。

負荷はカーネルメモリ破壊とは別問題だが、ホストの latency、メモリ圧迫、サービスの timeout に結びつく。ring の飽和や pending の不足は通常は観測欠落として処理される。file の入れ子深度・entry/exit の対応が崩れた場合も、まず集計ミスや pending の残留を疑うべきで、対象ファイルへの上書きと同一視できない。

**対策:** BPF 内での PID/cgroup フィルタ、file センサーの独立した無効化、path の送信頻度削減・集約、CPU 数に応じた map サイズを導入する。kernel map のメモリ会計が利用する制限と、user space の cgroup/FD 制限を実機で確認し、CPU の多いホストでも測定する。

### 4.3 attach/detach とエラー処理

[sched.rs](../src/http_server/system/sched.rs)、[ipc.rs](../src/http_server/system/ipc.rs)、[files.rs](../src/http_server/system/files.rs) は組み込みの BPF オブジェクトをロードし、`Link` と `Object` を所有する。HTTP 入力から任意の BPF ソースや object をロードする API はない。pin や永続的なリンク配置も見つからず、通常の drop/プロセス終了で所有するリソースを解放する構造である。途中の attach 失敗も、ローカルの `Vec<Link>` の drop で既に接続した分を解放する構造になっている。

[system/service.rs](../src/http_server/system/service.rs) はシステム購読者がいないと collector を drop する。初期化失敗は [activity.rs](../src/http_server/system/activity.rs) でセンサー単位の unavailable 状態になる。一方、poll/collect のエラーは状態を変更するだけで、生きているセンサーを自動 detach しない。異常表示や観測欠落が出ても、kernel hook の負荷が止まったとは限らない。

**対策:** 継続エラーや過負荷で対象センサーを停止する仕組みと、フックが解放されたことを確認する終了テストを追加する。通常の capability で十分かをカーネル別に確認し、ロード失敗を避けるために権限を無制限に広げない。

### 4.4 カーネル自体の不具合は別の残存リスク

CO-RE と BTF は構造体配置の違いに対応するが、接続先の意味や workload に対する安全性まで保証しない。正常な verifier が拒否するコードと、verifier/JIT/helper/trampoline 自体の不具合で通ってしまうコードは区別する必要がある。後者の影響は procinsh の Rust の型安全性では防げない。

今回、特定の CVE がこのバイナリや稼働ホストに成立するかは評価していない。カーネル更新の管理と、サポートする kernel/BTF の組み合わせごとの load/attach/負荷試験が必要である。カーネル障害の検証は本番ホストではなく使い捨て VM で行う。コンテナだけではカーネルをホストと共有するので、この種の障害を隔離できない。

## 5. perf: 対象の書き換えよりも資源と負荷

[perf.rs](../src/http_server/process/snapshot/perf.rs) は TID ごとに CPU-clock event と context-switch event を作る。CPU-clock は実行中の user CPU 時間に対して約100 Hz、context-switch 側は切り替えイベントに応じてユーザーレジスタと最大16 KiBのスタックを収集する。後者の負荷は100 Hzに固定されていない。

`mmap(PROT_READ | PROT_WRITE)` は **perf の共有 ring と metadata** を map するもので、対象のメモリを writable に map するものではない。`data_tail` を書くのは消費位置を知らせるためである。ring の配置・record 長・stack 長を検査し、head/tail に Acquire/Release を使い、失敗時はその ring のデータを捨てる。破損 record を対象メモリへ書き戻す経路はない。FD は `OwnedFd`、mapping は `Drop` で解放する。

ただし、4 KiB page の環境では、17 pages × 2 events = **TID あたり約136 KiB** の mapping が必要となる。1,000 TID なら1接続でも約133 MiBであり、32接続なら約4.15 GiB、FD は最大64,000個相当となる。これは全 event の作成に成功した場合の概算で、その他のメモリを含まない。実際は RLIMIT、perf のメモリ制限やカーネル設定によって先に失敗する場合もある。[Linux kernel: Perf events and tool security](https://docs.kernel.org/admin-guide/perf-security.html)

[monitoring/service.rs](../src/http_server/process/monitoring/service.rs) は同時接続を32に制限するが、接続ごとに独立した `Sampler` を作る。全 TID 数、全 perf FD 数、全 mapping 容量にはアプリケーション独自の総量上限がない。同一 PID を複数のブラウザ・接続で開いても重複する。ptrace の競合はエラーとして扱うが、perf は独立して動きうる。

**対策:** TID/FD/メモリの総量制限、PID ごとの collector 共有、context-switch event の無効化・頻度制御を設ける。32という接続数だけでは資源予算を表せない。接続を繰り返すと初回 ptrace が再実行されるので、PID ごとの停止頻度制限も必要である。

## 6. 強い権限を持つプロセスの攻撃面

### 6.1 「読取専用 API」は OS の権限制限ではない

[dev_run.sh](../scripts/dev_run.sh) は実行ファイルに `cap_sys_ptrace,cap_bpf,cap_perfmon,cap_dac_read_search=ep` を付ける。これらを持つ HTTP サーバーが侵害されると、現在の router に書き込み API がなくても、別の syscall を呼ぶことで対象メモリやレジスタを変更できる可能性がある。特に `CAP_SYS_PTRACE` は読み取り専用の capability ではない。`CAP_DAC_READ_SEARCH` は通常のファイル読み取り・ディレクトリの読み取りと探索に対する DAC の権限検査を迂回するので、侵害時の機密ファイル読み取りの影響も広がる。ただし、この権限単独で通常のファイル書き込み権限を迂回するものではなく、LSM 等の別の制約がなくなるわけでもない。root 起動は、さらに広い権限を与える。[capabilities(7)](https://man7.org/linux/man-pages/man7/capabilities.7.html)

HTTP、`/proc` の解析、libbpf の FFI、perf の unsafe、ELF/DWARF の解析を同じプロセスで行う。Rust は多くのメモリ破壊を防ぐが、unsafe・ネイティブライブラリ・依存クレートの欠陥まで自動的に排除しない。観測対象が作る実行ファイルや mapping の情報も、強い権限を持つ解析器に入る信頼できない入力である。今回、そこからコード実行できる具体的な欠陥を見つけたわけではない。

**対策:** HTTP/UI と特権 collector を別プロセスに分け、対象と操作を限定した IPC にする。ptrace が必要な worker と BPF worker の権限を分け、不要になった capability を落とす。継続的な再 attach/open が必要なので、現構造のまま起動直後にすべて落とせるとは限らない。seccomp を併用する場合も、汎用 `ptrace` / `bpf` を許可しただけで読取専用になると考えない。

### 6.2 setcap と自動更新も権限境界になる

file capability は一度付けると、通常の終了で消えない。実行ファイル・祖先ディレクトリのアクセス権によっては、他ユーザーも強い権限付きのバイナリを起動できる。loopback の HTTP にも OS ユーザーの認証はないので、ローカルの別ユーザーからの利用を禁止するものではない。

[main_preview.sh](../scripts/main_preview.sh) は候補を mode `0755` で配置し、capability を付けて更新する。[MAIN_PREVIEW.md](MAIN_PREVIEW.md) の sudoers 例はユーザー所有の candidate パスへの `NOPASSWD setcap` を許す。そのパスに別の実行ファイルを置けるユーザーは、文書通りのルールならそれにも同じ capability を付与できる。これはルールが procinsh の内容・ハッシュを検証しないことから導かれる権限上の意味であり、稼働ホストにそのルールが存在することを確認したわけではない。

**対策:** バイナリと配置ディレクトリの owner/mode、capability、sudoers を確認し、実行ユーザーを限定する。特権を付ける成果物を root 管理の配備経路で検証する。最新 main の自動ビルドへそのまま権限を付ける運用は、リポジトリ・ビルド依存・更新経路まで信頼することを意味する。常設する場合はレビュー済みの版を固定する。

## 7. ELF/DWARF と通常 HTTP API の DoS

[symbol/cache.rs](../src/http_server/process/snapshot/symbol/cache.rs) はファイルサイズを512 MiB、cache 件数を64に制限するが、**解析後の総メモリ量**を制限しない。多数の大きな ELF、展開された debug section、複数接続分の cache が重なると、大きなメモリ消費となりうる。件数上限だけではバイト数の上限にならない。

さらに `matching_file` は pathname の metadata で inode/device を確認した後、`ElfSymbols::load` がその pathname を再度開く。確認した FD を使い続ける方式ではないので、ファイルの増大・置換・対象の exec との TOCTOU が残る。`fs::read` は確認済みサイズを上限として読むものではない。fallback の `/proc/PID/root/...` などでファイルが置換されると、確認とは異なるファイルへの I/O になる可能性がある。成立条件は未再現だが、対象が管理する filesystem、FUSE、ネットワーク filesystem の遅い I/O も timeout なしの解析には問題となりうる。

通常の詳細 API も `spawn_blocking` を利用し、SSE の32接続制限とは独立している。[api/process.rs](../src/http_server/api/process.rs) に共通の同時実行数・頻度制限はない。[maps.rs](../src/http_server/process/maps.rs) の maps/smaps 読み取りは `read_to_string` でサイズ上限を設けていない。大量の mapping を持つ対象への繰り返し要求は追加負荷になる。環境変数1 MiB、auxv 64 KiB、FD探索の件数・時間制限など、個別の防御はあるが全 API の総量を制限するものではない。

**対策:** 同じ FD 上で file type・identity・size を検査し、読み取りと debug 展開にもバイト上限を付ける。cache の総容量と解析の同時実行数を制限し、重い解析を低権限 worker に分離する。通常 API にも共有 semaphore・頻度制限・実行期限を設ける。

## 8. 既知の公開リスクとの関係

`0.0.0.0` 公開時の環境変数漏えいに加え、到達できるクライアントは SSE の GET 要求で上述の ptrace/perf を開始できる。したがって、非 loopback 公開の問題は情報漏えいだけではなく、**強い権限を持つ観測操作を他者が起動できること**でもある。任意アドレスのメモリ読み取り HTTP API があるわけではないが、レジスタ・スタックフレーム・命令 bytes・maps 等の情報は返る。

[middleware.rs](../src/http_server/middleware.rs) の Host/Origin/Fetch Metadata 検査はブラウザ経由の攻撃に対する防御になるが、任意の header を付けられる HTTP クライアントの認証ではない。既定 loopback と `--allow-non-loopback` の明示要求は有効な防御である一方、[常設プレビューの service](../scripts/systemd/procinsh-preview.service) は `0.0.0.0` を明示して起動する。リバースプロキシを置く場合は認証/TLSに加え、裏側ポートへ直接到達できない配置にする必要がある。

## 9. 対策と追加検証の順番

1. **対象を止めない選択肢を用意する。** ptrace の有無を分離し、重要なプロセスは perf のみ、または軽い `/proc` 観測だけにする。現 CLI にこの無効化オプションはない。
2. **停止状態の保存を検証する。** 使い捨て fixture で、観測前の SIGSTOP、捕捉中の SIGSTOP/SIGCONT、SIGTSTP、通常シグナル、exec、スレッド終了を試す。観測後の停止状態、実際のシグナル受信、アプリの進行、`TracerPid` を確認する。
3. **待機と回復を検証する。** 長い停止待ち・遅いメモリ取得・取得失敗・detach 失敗・接続切断・観測側終了を対象に、待機時間と停止時間を計測する。関数の終了だけでなく、対象が元の状態に戻ったことを確認する。
4. **総資源を制限する。** 多数の TID、同一 PID の複数接続、再接続、短い file/socket I/O、高 context-switch、CPU 数の多い VM で、対象の処理時間・ホスト latency・FD・RSS・kernel map の量・lost を測定する。上限超過時に新規観測を拒否する。
5. **強い権限を持つ部分を小さくする。** 特権 collector の分離、対象 allowlist、低権限での ELF/DWARF 解析、配備パスと sudoers の監査を行う。
6. **kernel ごとの試験を行う。** ロード失敗だけでなく、部分 attach 失敗、購読終了、センサー異常、プロセス終了後の link/map 解放を確認する。

このリポジトリの権限付き Rust テストは `./scripts/dev_test.sh` を使い、perf の検証では `PROCINSH_REQUIRE_PERF=1 ./scripts/dev_test.sh` として権限不足によるスキップを禁止する。サーバー起動を含む対応する結合テストは `PROCINSH_BINARY=./scripts/dev_run.sh` を使う。障害・負荷系の追加検証は、重要なプロセスのない使い捨て VM で行う。

対策実装前の運用では、観測対象と接続者を信頼できる範囲に絞り、短い停止でも困るプロセスの詳細 SSE を開かない。loopback は外部からの到達を減らすが、ptrace の意味や eBPF の負荷を変えない。
