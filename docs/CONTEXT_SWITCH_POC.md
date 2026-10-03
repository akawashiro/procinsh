# Context-switch perf PoC (#107)

本体の CPU-clock collector は変更せず、opt-in の ignored Rust test で
`PERF_TYPE_SOFTWARE / PERF_COUNT_SW_CONTEXT_SWITCHES` を TID ごとに開く。
period=1、TID/TIME/CPU/REGS_USER/STACK_USER、CLOCK_MONOTONIC を使用する。
CPU-clock と異なり exclude_kernel/exclude_hv を設定しない。
context_switch と sample_id_all を指定し、SWITCH の trailer に TID/time/CPU を付ける。
`PROCINSH_POC_SWITCH=0` で SWITCH record を無効にした比較もできる。

既存の RegisterSet、perf parser/ring buffer、frame-pointer unwind、ELF symbol
resolver を再利用する。ptrace や process_vm_readv は使用しない。

## 実行

```sh
cargo test --locked --bin procinsh --no-run
# Executable として出力された検証バイナリを dev_run.sh の起動先に置く。
# 元の本体を保存し、hard link の先を上書きしないよう remove-destination を使う。
cp target/debug/procinsh /tmp/procinsh-before-context-switch-poc
cp --remove-destination target/debug/deps/procinsh-<hash> target/debug/procinsh
python3 scripts/context_switch_poc.py ./scripts/dev_run.sh \
  --mode sleep --repeats 3 |& tee /tmp/context-switch-sleep.log
# 検証終了後に通常の本体へ戻す。
cp --remove-destination /tmp/procinsh-before-context-switch-poc target/debug/procinsh
```

`dev_run.sh` が CAP_SYS_PTRACE/CAP_BPF/CAP_PERFMON を設定して実行する。
検証中の `target/debug/procinsh` は HTTP server ではなく Rust test harness となる。
測定中は再ビルド・バイナリの差し替えをしない。

`--mode socket / fsync / busy / initial / oneshot` でも同じ matrix を実行する。
fixture は -O2 -g -fno-omit-frame-pointer -fno-optimize-sibling-calls でビルドする。
`--omit-frame-pointer` で frame pointer omission を比較する。
`initial` は接続前から10秒 sleep し、観測窓の間に復帰しない。
その他は8秒実行し、開始約200ms後から既定5秒間観測する。
`oneshot` は開始500ms後に6秒 sleep する。観測開始後の待機遷移が取れることを確認する。
file I/O は既定で tmpfile + fsync。`--fsync-directory target` で指定 filesystem 上に
一時ファイルを作り、直ちに unlink して使用する。filesystem を結果とともに記録する。
`--configs clock context-2048 both-8192` のように比較項目を絞ることもできる。

比較する構成は baseline、CPU-clock のみ、context の 2/4/8 KiB、
CPU-clock + context 8 KiB、SWITCH 無効の context 8 KiB。
各構成の target iterations/wall/cpu を比較し、複数回の中央値と範囲を報告する。
観測は target の実行時間の一部なので、差は8秒全体に対する影響であり、
観測区間だけの overhead として扱わない。observer_cpu_ns、ring_bytes、lost も報告する。
1ms polling の observer コストも含む PoC の測定であり、製品 collector の overhead ではない。
observer_cpu_ns は polling/drain の process CPU 時間であり、event open・maps 取得・
最後の symbol 解決は含まない。first_usable_after_poll_ms は polling 開始から初回の usable
sample を読み出すまで、last_sample_age_ms は最後の sample から観測終了までの時間。
State/TracerPid を観測前後に出力する。

単一 TID の手動観測:

```sh
PROCINSH_POC_TID=123 PROCINSH_POC_STACK=4096 PROCINSH_POC_SECONDS=5 \
  ./scripts/dev_run.sh context_switch_poc --ignored --nocapture
```

CPU-clock を併用するには `PROCINSH_POC_CLOCK=1`、CPU-clock のみは
`PROCINSH_POC_CLOCK_ONLY=1`。新規 TID の追加・PID 再利用追跡は本 PoC の対象外。

## 結果の読み方

- samples は raw SAMPLE 全件。abi_none は REGS_USER ABI_NONE の件数。
  ABI_NONE を usable sample に数えず、最後の usable sample の RIP/RSP/RBP、
  stack_bytes、symbol 付き frames、unwind_stop を表示する。
- SWITCH の misc の SWITCH_OUT (bit 13) と SWITCH_OUT_PREEMPT (bit 14)
  を用いて in / voluntary out / preempt out を数える。
  sample 自体から voluntary/preempt を断定しない。
- 最初の32件の `(record kind, misc, time_ns)` を出力する。
  SAMPLE と SWITCH の時刻・順序を比較する。常に一対一になるとは仮定しない。
- ring は既存と同じ64 KiB。高頻度対象では lost/overrun を必ず確認する。
  overrun / unsupported ABI / permission denied はテスト失敗として報告する。
- ABI_NONE と sample 自体がない場合、stack_bytes=0 と unwind 失敗を区別する。
  CPU-clock と比べ、sleep の開始直後に usable sample を得るかを確認する。
  initial では観測開始時に過去の待機遷移を遡って取得できないことを確認する。

## 実測 (2026-10-03)

Linux x86-64 7.0.0-15-generic / PREEMPT_DYNAMIC、AMD Ryzen 9 5950X
(32 logical CPUs)、perf_event_paranoid=4。`dev_run.sh` を使用して55 runを実行した。
集計前の各 run の数値、register、frame symbols、最初の6 record の時刻は
[測定データ](experiments/context-switch-2026-10-03.json) に保存している。
スタックの生メモリは保存していない。

### 待機時の取得と分類

- 1ms sleep は5秒間で約4,727 sample。2/4/8 KiB の全9 run で15 frameを復元し、
  `clock_nanosleep → __nanosleep → wait_once → nested ×7 → main` を確認した。
  ABI_NONE/lost は0。CPU-clock 単独と併用側の CPU-clock は0 sampleだった。
- socketpair の blocking recv は8 KiBで4,725 sample / 14 frame、lost=0。
  `__recv → recv → nested ×7 → main` を確認。
  併用時は voluntary=4,727 / preempt=1、CPU-clock=2 sample。
- ext4 (NVMe) の write/fsync は8 KiBで14,831 sample、voluntary out=21,425 /
  preempt out=16、lost=6,610。併用時も14,907 sample / lost=6,494。
  `fsync → wait_once → nested ×7 → main` の14 frameを確認した。
- 同じ ext4 の2 KiBは21,568 sample / lost=0、4 KiBは21,222 sample / lost=0。
  いずれも14 frameで main まで復元できた。実際の STACK_USER used bytes は
  requested size 以下（mapped stack の終端にも依存する）。大きくするだけで精度が上がるとは限らない。
  各 fsync サイズは1 runのみであり、欠落率の普遍的な保証ではない。
- tmpfs の fsync は voluntary=0、preemptのみ50/63件。CPU-clock は227件。
  これは disk wait の例ではなく、実ディスクの測定と区別する。
- CPU busy は各 context run で53–64 preempt out、voluntary=0。
  併用側の CPU-clock は各499 sample。preempt の bit 14 は実際に観測できた。
- 最初の sleep sample は misc=1（kernel event）でも x86-64 user ABIのregister/stackを持った。
  例: sample time=344592545630228ns、直後の voluntary SWITCH_OUT
  (misc=8192) time=344592545633084ns。差は2,856ns。
  `exclude_kernel` を設定せず採取する理由を裏付ける。
  これはこのカーネルの観測であり、他のカーネルでも同じ record 順序とは仮定しない。

### 睡眠継続・未取得・frame pointer omission

`oneshot` は接続後に6秒 sleep へ入る。2 KiBで初回 usable sample は polling開始後
272.512ms、併用8 KiBでは281.184ms。その後の sample はなく、観測終了時に
sample age=4,728 / 4,720ms、State=S、TracerPid=0のまま15 frameを復元した。
待機中に CPU-clock が来るのを待たず、遷移時の sample を表示に使える。
CPU-clock 単独は0 sampleだった。初回時刻は観測開始からの時間であり、
待機遷移から取得までのレイテンシではない（対象は起動500ms後に待機する）。

`initial` は接続前から10秒 sleep し、5秒の観測窓には遷移しない。
CPU-clock / context / 併用の全てが sample=0、SWITCH=0。
既に眠っている対象の過去の遷移は遡って取得できない。

-fomit-frame-pointer では2/8 KiBとも register/stack は取れたが、4 frameで停止し
`RBP outside stack / unaligned (frame pointers may be omitted)` を報告した。
main/nestedは復元できず、stack bytes を増やしても解決しなかった。
ABI_NONE の synthetic parser test は usable sample に数えないことを検証する。
今回の実機 run で ABI_NONE は発生しなかった。

通常権限では CPU-clock / context とも perf_event_open が EACCES (os error 13) で失敗する。
権限不足を空の観測の成功として扱わない。ptrace 呼び出しはなく、
状態診断を追加した fsync/oneshot run の TracerPid は全て0だった。

### オーバーヘッド

以下は各構成3回、固定順序、CPU pinningなしの中央値と範囲。
対象は8秒、観測は約200ms後から5秒。throughput は target の iterations。
CPU time の単位は秒。対象と観測者の CPU 時間を別々に示す。

1ms sleep:

| 構成 | target iterations 中央値 (min–max) | target CPU 中央値 | observer CPU 中央値 (min–max) |
|---|---:|---:|---:|
| baseline | 7,575 (7,574–7,576) | 0.036384 | 0.000000 (0.000000–0.000000) |
| clock | 7,572 (7,571–7,572) | 0.040763 | 0.023965 (0.022824–0.024103) |
| context-2048 | 7,568 (7,567–7,571) | 0.043494 | 0.243847 (0.243362–0.246028) |
| context-4096 | 7,567 (7,566–7,572) | 0.043979 | 0.418802 (0.417518–0.419662) |
| context-8192 | 7,568 (7,567–7,568) | 0.043944 | 0.726873 (0.724498–0.727176) |
| both-8192 | 7,568 (7,565–7,569) | 0.045022 | 0.731550 (0.725430–0.731818) |
| no-switch-8192 | 7,572 (7,570–7,573) | 0.040732 | 0.728502 (0.722106–0.738598) |

CPU busy:

| 構成 | target iterations 中央値 (min–max) | target CPU 中央値 | observer CPU 中央値 (min–max) |
|---|---:|---:|---:|
| baseline | 382,500 (382,276–382,904) | 7.999391 | 0.000000 (0.000000–0.000000) |
| clock | 382,187 (381,471–382,430) | 7.999002 | 0.097869 (0.095718–0.100788) |
| context-2048 | 382,175 (379,285–382,263) | 7.998815 | 0.027366 (0.027064–0.028144) |
| context-8192 | 382,090 (381,959–382,395) | 7.998764 | 0.032044 (0.030731–0.033271) |
| both-8192 | 382,353 (381,816–382,354) | 7.998900 | 0.107663 (0.106617–0.107744) |

sleep の併用は baseline 比で処理量が約0.09%低下、target CPU が約8.6ms増えた。
observer CPU は CPU-clock の約24msから約732msに増え、5秒の約14.6%に相当する。
context の2/4/8 KiBの observer CPUは約244/419/727ms。
ring bytes は5秒で約10.9/20.6/40.0MB。この PoC の既存 ring copy はbyteごとの
volatile readを行うため、このコストも含まれる。

CPU busy の併用と baseline の throughput 差は約0.04%で、run の範囲は重なる。
3回だけの測定から有意な性能差とは断定しない。
observer CPU は CPU-clock の約98msから併用の約108msに増えた。
context switch の少ない busy と頻繁に待機する workload でコストは大きく異なる。

### #106 への判断材料

このカーネルでは、CPU-clock を on-CPU、context-switch を待機遷移に使う方式は実測上成立する。
2 KiBを初期候補として、実運用の深い stack、ring 容量、poll 周期、copy のコスト、
sample と SWITCH の対応・欠落時の分類を別途設計する。
本 PoC は最後の usable sample だけを unwind し、各 sample を継続的にsymbolizeする負荷は測っていない。
本体へ導入する際の負荷をこの数値だけで保証しない。
観測前から sleep 中の thread と frame pointer omission は残る制約であり、
#106 の fallback 判断に使える。

検証は通常の `cargo test --locked --bin procinsh` が63成功・1 ignored、
`PROCINSH_REQUIRE_PERF=1 ./scripts/dev_run.sh live_tests --nocapture` が2成功（skipなし）。
capability付き harness の全テスト実行では自己プロセスの environment API検証が
422を返し、62成功・1失敗・1 ignoredだった。通常実行では同じテストが成功する。
PoC 本体と既存 live perf の確認は capability付きで実施した。
Clippy、Rust/C整形、rustdocリンク検査、Cのwarning-as-errorビルドも実施した。
