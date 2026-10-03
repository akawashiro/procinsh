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
# 上の出力にある Executable のパスを使用する。perf 権限を持つ実行環境が必要。
sudo python3 scripts/context_switch_poc.py target/debug/deps/procinsh-<hash> \
  --mode sleep --repeats 3 |& tee /tmp/context-switch-sleep.log
```

`--mode socket / fsync / busy / initial` でも同じ matrix を実行する。
fixture は -O2 -g -fno-omit-frame-pointer -fno-optimize-sibling-calls でビルドする。
`--omit-frame-pointer` で frame pointer omission を比較する。
`initial` は接続前から10秒 sleep し、観測窓の間に復帰しない。
その他は8秒実行し、開始約200ms後から既定5秒間観測する。
file I/O は tmpfile + fsync を使うため、filesystem を結果とともに記録する。

比較する構成は baseline、CPU-clock のみ、context の 2/4/8 KiB、
CPU-clock + context 8 KiB、SWITCH 無効の context 8 KiB。
各構成の target iterations/wall/cpu を比較し、複数回の中央値と範囲を報告する。
観測は target の実行時間の一部なので、差は8秒全体に対する影響であり、
観測区間だけの overhead として扱わない。observer_cpu_ns、ring_bytes、lost も報告する。
1ms polling の observer コストも含む PoC の測定であり、製品 collector の overhead ではない。

単一 TID の手動観測:

```sh
PROCINSH_POC_TID=123 PROCINSH_POC_STACK=4096 PROCINSH_POC_SECONDS=5 \
  target/debug/deps/procinsh-<hash> context_switch_poc --ignored --nocapture
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

## この checkout での検証 (2026-10-03)

Linux x86-64 7.0.0-15-generic / PREEMPT_DYNAMIC、perf_event_paranoid=4。
通常権限の CPU-clock perf_event_open は EACCES (os error 13) で失敗した。
権限を付けた実測は `sudo -n` が interactive authentication required で失敗したため未実施。
したがって context sample / user registers / SWITCH classification / overhead / initial
の実測結果は未確定であり、#107 の完了条件を満たしたとは扱わない。
この PoC だけを根拠に #106 の方式は選定しない。
