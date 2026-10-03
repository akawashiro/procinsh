# Syscall tracepoint perf PoC (#109)

native Linux x86-64 の raw_syscalls:sys_enter / sys_exit を TID 単位で開き、
syscall entry の user register/stack と kernel 内の滞在時間を取得する実験。
本体の collector/UI は変更しない。既存の RegisterSet、frame-pointer unwind、
ELF symbol resolution、perf ring を再利用する。ptrace/process_vm_readv は使用しない。

## 実行

```sh
cargo test --locked --bin procinsh --no-run
# 上の Executable パスを使用する。dev_run.sh の起動先を一時的に test harness に置き換える。
cp target/debug/procinsh /tmp/procinsh-before-syscall-poc
cp --remove-destination target/debug/deps/procinsh-<hash> target/debug/procinsh

# ID を識別してから全構成を測定する。
python3 scripts/syscall_tracepoint_poc.py --mode sleep --repeats 3
# 既に同じ boot の ID を確認した場合は --enter-id N --exit-id M を指定して探査を省略できる。
python3 scripts/syscall_tracepoint_poc.py --mode futex --repeats 1 \
  --configs all-2048 blocking-2048 with-context-2048 --wait-ms 6000

# 終了後は通常の本体に戻す。dev_run.sh は本体の capability を再設定する。
cp --remove-destination /tmp/procinsh-before-syscall-poc target/debug/procinsh
```

launcher は既定で `./scripts/dev_run.sh`。CAP_SYS_PTRACE/CAP_BPF/CAP_PERFMON を付ける
既存手順を使う。検証中のバイナリは HTTP server ではない。測定中に再ビルド・差し替えはしない。

この環境の tracefs id/format は root:root 0440 で読み取れない。
ID 探査は自分の TID にだけ tracepoint を開き、`id == 39` (getpid) の kernel filter と
既知の getpid の RAW payload を使う。entry の args[6] と exit の PID 戻り値を識別する。
tracefs、perf_event_paranoid、sudoers は変更しない。ID は kernel/boot に依存し、固定値として使わない。
RAW layout は実行 kernel の BTF (`trace_event_raw_sys_enter`, `trace_event_raw_sys_exit`,
`trace_entry`) と実際の common_type/common_pid を検査して確認した。

`--mode` は sleep/futex/epoll/busy/initial。`--wait-ms` は1〜8000、既定1000。
fixture は -O2 -g -pthread -fno-omit-frame-pointer -fno-optimize-sibling-calls。
`--omit-frame-pointer` は同じソースを -fomit-frame-pointer で比較する。
対象は PID 出力後300ms sleep し、その後約8秒まで実行する。長い wait は終了が8秒を越える。
`initial` は観測前から10.3秒 sleep する。既定の観測窓は起動約100ms後から5秒。

## 取得・対応付け

- enter は PERIOD=1、TID/TIME/CPU/RAW/REGS_USER/STACK_USER。exit は既定で
  TID/TIME/CPU/RAW のみ。`exit-stack-8192` は enter/exit 両方で register/stack を取る。
  CLOCK_MONOTONIC、kernel/hypervisor を除外しない。filter を設定してから enable する。
- RAW は common trace header の後に native 64-bit syscall ID。
  enter は args[6]、exit は signed return value。RAW size の alignment padding を消費してから
  user ABI/register/stack を解析する。ABI_NONE でも syscall metadata は保持する。
- syscall 名は native x86-64 の blocking 候補と workload の番号表を使う。
  未登録番号は `syscall_N` として番号を残す。32-bit compat の名前・register は対象外。
- enter/exit は2本の ringをmonotonic timestamp順にマージし、TID と ID の両方で照合する。
  poll開始後の時刻の record は次の poll に持ち越し、片方のdrain中に生成されたexitが
  まだ読んでいないenterを追い越すのを防ぐ。遅着・不一致・初回のunmatched exitは診断する。
- LOST または THROTTLE/UNTHROTTLE を検出した poll の対応付けと pending を破棄する。
  消えたexitを長時間滞在と誤認しない。観測終了後のkernel状態を厳密に同期した表示ではなく、
  最新の処理済みrecordまでの観測である。最後のcutoff以降のqueueは別途数える。
- 500ms以上exitを観測していないentryは `IN_SYSCALL` として途中経過を出す。
  scheduler上のoff-CPUや待機原因とは同一視しない。context併用はcounterの比較であり、
  syscall/switchの時間軸をjoinしてoff-CPU区間を算出する機能は実装していない。
- duration は kernel enter/exit tracepoint の差で、kernel CPU処理、睡眠、preemption、
  本PoC自身のsamplingコストを含む。ユーザー空間へ戻るまでの全時間や純粋なsleep時間ではない。
  signal/restart の中間exit・再enterは個別attemptとして扱う。restartはsynthetic testで検証し、
  実機signal injectionは今回の測定対象外。
- pending entry の stack を優先し、なければ最新の完了したentryのstackを表示する。
  `sample_source`/timestamp/age を付け、過去のstackを現在のpendingとして扱わない。
  最後の1 sampleだけをunwind/symbolizeする。全entryのsymbolize負荷は測っていない。

構成一覧:

| 構成 | enter | exit | kernel filter |
|---|---|---|---|
| baseline | なし | なし | なし |
| light | RAW metadata | RAW metadata | 全syscall |
| all-2048/4096/8192 | RAW + register/stack | RAW metadata | 全syscall |
| blocking-2048/4096/8192 | RAW + register/stack | RAW metadata | blocking候補 |
| exit-stack-8192 | RAW + register/stack | RAW + register/stack | 全syscall |
| context-2048 | #107と同じcontext-switchのみ | — | — |
| with-context-2048 | all-2048 + context-switch | RAW metadata | 全syscall |

light の RAW にはtracepoint固定payloadのargs/returnも含まれる。
perf RAWはフィールドを射影できないため、厳密な「ID/timestampだけ」の小さいpayloadではない。
blocking候補には futex/epoll/poll/select/read/recv/accept/connect/nanosleep/wait/io_uring_enter と
fsyncを含める。候補に入っているだけで待機とは判定しない。

## 実測 (2026-10-03)

Linux 7.0.0-15-generic x86-64 / PREEMPT_DYNAMIC、Ryzen 9 5950X (32 logical CPUs)、
perf_event_paranoid=4。dev_run.sh で63 run を実行した。
このbootで識別したenter/exit IDは384/383。
[各runの数値・register・frame symbols](experiments/syscall-tracepoint-2026-10-03.json)を保存した。
stackの生メモリは保存していない。

### 待機時のcall pathと時間

1秒待機の3構成（all-2048 / blocking-2048 / with-context-2048）では各5 enter/5 exitを採取し、
各4回のdurationを対応付けた。最初の300ms sleepは観測開始前に入るため、そのexitは
unmatched=1として残し、過去のentry/stack/durationを推測しない。lost/ABI_NONE/order_errorsは0。

| workload | ID | 1秒設定での平均滞在時間（構成による範囲） | return | frame数 / call path |
|---|---:|---:|---:|---|
| nanosleep | clock_nanosleep (230) | 1000.068〜1000.072ms | 0 | 15 / clock_nanosleep → __nanosleep → wait_sleep → nested ×7 → main |
| pthread condition wait | futex (202) | 1000.063〜1000.075ms | -110 (ETIMEDOUT) | 15 / pthread_cond_timedwait → wait_futex → nested ×7 → main |
| epoll timeout | epoll_wait (232) | 1000.746〜1001.021ms | 0 | 14 / epoll_wait → wait_epoll → nested ×7 → main |

6秒待機のall-2048では各1 enterを取得し、観測終了時にclock_nanosleep=4,828ms、
futex=4,822ms、epoll_wait=4,824msのpendingを表示した。それぞれuser RIP/RSP/RBPと
15/15/14 frameを保持し、State=S、TracerPid=0だった。raw argumentsも取得できた。
終了していないsyscallの「観測済み滞在中」とcall pathを示せる。

-fomit-frame-pointerでは2/8 KiBともRAW/register/stackは取れ、duration対応も成立したが、
4 frame（clock_nanosleep / __nanosleep / wait_sleepまで）でchainが終了し、main/nestedは復元できなかった。
このcaseのunwind_stopは `end of chain / non-increasing RBP`。
単に正常終了に見えるchainでもcallerが欠け得るため、完全なcall pathの保証にはしない。

initialではenter/exitとも0、pendingもuser sampleもなし。syscall entryを観測する前から
待機している対象には情報がない。通常権限のperf_event_openはEACCESで失敗し、空の成功にしない。
今回の実機ではABI_NONEとTHROTTLEは発生しなかった。ABI_NONE・truncation・異なるTID・
欠落・不一致・遅着・overwrite・restart returnの扱いはunit testでも確認している。

### 観測負荷

各構成3回の中央値とmin–max、固定順序・CPU pinningなし。対象は約8秒、観測は5秒なので、
throughput差は8秒全体への影響であり、観測窓だけのslowdownではない。
observer CPUはring copy/RAW/register parsing/merge/対応付け/途中経過のCPU時間で、
event open、maps取得、最後のsymbol解決を含まない。FDは準備から最後の表示まで有効なため、
target throughputには準備・後処理中のtracingの影響も含まれる。ringは1 eventにつき64 KiB、pollは1ms。

1ms nanosleep:

| 構成 | target iterations 中央値 (min–max) | observer CPU 秒 中央値 (min–max) | enter lost 中央値 |
|---|---:|---:|---:|
| baseline | 7,292 (7,291–7,293) | 0.000000 (0.000000–0.000000) | 0 |
| light | 7,276 (7,191–7,277) | 0.053977 (0.052914–0.054825) | 0 |
| all-2048 | 7,272 (7,271–7,273) | 0.243827 (0.242965–0.248350) | 0 |
| all-4096 | 7,264 (6,734–7,268) | 0.408902 (0.408020–0.410742) | 0 |
| all-8192 | 7,265 (7,263–7,267) | 0.683648 (0.678758–0.685994) | 15 |
| context-2048 | 7,285 (7,285–7,286) | 0.227408 (0.226463–0.228068) | 0 |
| with-context-2048 | 7,265 (6,647–7,269) | 0.436369 (0.413510–0.436908) | 0 |

getpid busy-loop:

| 構成 | target iterations 中央値 (min–max) | observer CPU 秒 中央値 (min–max) | enter lost 中央値 |
|---|---:|---:|---:|
| baseline | 23,810,622 (23,708,742–23,917,931) | 0.000000 (0.000000–0.000000) | 0 |
| light | 12,745,376 (12,716,167–12,806,873) | 3.559530 (3.559163–3.561364) | 3,202,058 |
| all-2048 | 13,087,735 (13,069,165–13,097,821) | 3.399953 (3.398481–3.404159) | 4,228,712 |
| all-4096 | 13,145,442 (13,066,867–13,159,212) | 3.386238 (3.382834–3.390771) | 4,256,144 |
| all-8192 | 13,069,042 (13,045,768–13,119,031) | 3.373931 (3.370984–3.378180) | 4,268,075 |
| blocking-2048 | 15,302,974 (15,295,870–15,338,906) | 0.022731 (0.022482–0.025136) | 0 |
| exit-stack-8192 | 14,150,641 (14,134,988–14,153,950) | 3.016389 (3.014716–3.050668) | 5,339,017 |
| context-2048 | 23,780,587 (23,705,410–23,944,169) | 0.026312 (0.026142–0.026589) | 0 |
| with-context-2048 | 13,057,551 (13,044,746–13,062,713) | 3.397089 (3.393186–3.398657) | 4,191,646 |

1ms sleep のobserver CPU中央値はlightで54ms、2/4/8 KiBで244/409/684ms。
contextのみは227ms、2 KiB併用は436msだった。8 KiBのenter lostは0/58/15。
2/4 KiBは全6 runでlost=0だった。target側のCPU中央値はbaseline 33.9msから
all-2048で55.1ms、all-8192で61.4msへ増えた。
throughput中央値の低下はall-2048約0.27%、all-8192約0.37%、併用約0.37%。
ただし一部runには大きな外れ値があり、その原因は切り分けていない。中央値だけで性能保証しない。

getpid loopの8秒全体のthroughputはbaseline比でlight約46.5%、allの2/4/8 KiB約45.0/44.8/45.1%、
blocking-2048約35.7%低下した。getpidはblocking filterに含まれずenter sample=0でも、
tracepoint hookとkernel filterのコストは消えない。contextのみの差は約0.13%でrunの範囲が重なる。
observer CPUはlightで約3.56秒（5秒窓の約71%の1コア）、allで約3.37〜3.40秒、
blocking filterでは約23ms、contextのみでは約26msだった。

busyのall-2048はenter約38,000件に対してlost約4.2百万件。lightもenter約755,000件に対して
lost約3.2百万件だった。exit側にも数百万の欠落があり、pendingを破棄するinvalidationsが
繰り返し発生した。durationの残った少数のpairは全体の代表値ではなく、集計対象に選択の偏りがある。
exit-stack-8192が一部構成よりthroughput/observer CPUで軽く見えても、ほぼ全件を失っているので
効率のよい取得とは解釈しない。THROTTLEは全測定で0だった。

本PoCはdebug buildで、既存ringのbyteごとのvolatile copy、RAW解析、queueのソート、
BTreeMapへの対応付けのコストも含む。collector最適化やrelease buildで改善する可能性はあるが、
今回の測定から改善率を推測しない。複数TIDの実機観測や全sampleのsymbolizationは測っていない。

### #107との比較・#106への判断材料

[#107 のPoC（PR #108）](https://github.com/akawashiro/procinsh/pull/108)と同じperf/context設定を、
今回のfixtureでもcontext-2048として再測定した。

| 観点 | syscall enter/exit | context-switch |
|---|---|---|
| user register/stack | このkernelでentry時に取得できた | このkernelでswitch-out時に取得できた |
| 説明情報 | syscall ID/name、6 args、return、kernel滞在時間 | voluntary/preemptのswitch-out |
| 状態の意味 | exit未観測のsyscall内にいるという観測 | scheduler上の切り替えという観測 |
| 既に待機中で遷移なし | 過去のenterを取得できない | 過去のswitch-outを取得できない |
| frame pointer omission | callerが欠ける | callerが欠ける |
| 高頻度getpid loop | 大きいtarget負荷・大量のring loss | switchが少なく、負荷も小さい |
| 1ms sleep | entry/exitが多く、2 KiBでもobserver CPU約244ms/5秒 | 約227ms/5秒 |
| 2 KiB併用 | syscall説明情報とswitch counterを得るが、約436ms/5秒 | 単独よりコストが増える |

長いfutex/epoll/nanosleepを説明する情報源として成立する。syscallを待機と断定せず、
entry/exitと欠落状態を保持し、context観測でoff-CPUの意味を補完する設計が必要。
一方、全syscall + register/stackの常時記録は高頻度対象で負荷・欠落が大きく、
RAW metadataだけでも今回のperf転送方式では重かった。
候補filter + 小さいstack、対象/頻度の制限、kernel側集約などの方式を比較してから#106で選定する。
候補filterだけで高頻度対象への影響が小さくなるとは保証しない。
観測開始前からsleep中の対象や欠けたcall pathへのfallbackは引き続き必要。

検証: Rust単体テスト65成功・3 ignored、capability付き既存live perfテスト2成功（skipなし）、
ID probe成功、63 runの測定成功。Clippy全targets、Rust/C整形、C fixture warning-as-error、
rustdoc broken intra-doc linksも成功した。
