# procinsh: ptrace から perf_event_open への完全移行計画

対象: Linux x86-64 / Rust / 既存の Axum + SSE + TypeScript UI
関連: [Issue #7](https://github.com/akawashiro/procinsh/issues/7)
設計方針: **停止を伴う snapshot を廃止し、時刻付きライブサンプリングに一本化する。**

## 実装範囲の確定（2026-09-26）

この文書の移行方針に従って停止取得を削除する。Issue #7 の初期案にある停止 snapshot の併存は採用しない。今回の範囲は IP/TID/TIME/CPU、REGS_USER、ユーザー CALLCHAIN、SSE、UI、逆アセンブル、旧方式削除と開発資料更新まで。ユーザーの指定により STACK_USER とコピーしたスタックの巻き戻しは後続課題とし、DWARF unwind も含めない。以下の段階的な設計案は移行経緯として残し、現在の動作は `docs/DEVELOPMENT.md` と末尾の検証記録を参照する。

実装上の選択：MONOTONIC と remove_on_exec を必須とし、非対応カーネルで別の時計を単調時刻と偽装しない。対応下限は Linux 5.13。1サンプルの詳細上限64 KiB、stale表示は3秒超。設定は `--sample-hz`（既定49、1–199）と `--no-callchain`。exec 後は全詳細接続を閉じて開き直すまで収集を停止する。

## 1. 目的と対象外

### 目的

- 対象プロセスを停止せず、スレッド単位に RIP・汎用レジスタ・コールスタックを継続観測する。
- 既存の `/proc` によるプロセス情報・メモリマップ・FD・シグナル状態、`process_vm_readv()` による任意アドレスの読み取り、eBPF によるシステム活動収集は維持する。
- 観測結果を既存のプロセス詳細 SSE に追加し、サンプルの取得時刻・鮮度・欠落を UI で明示する。
- 同じプロセスを複数タブで開いてもサンプリングを重複させない。
- `ptrace` と `POST /api/processes/snapshot` を最終的に削除する。

### 対象外

- 全スレッドを停止した整合性のあるスナップショット。
- 指定した瞬間のレジスタ取得、シングルステップ、ブレークポイント、プロセスの停止・再開。
- 初期版での DWARF による完全なスタック巻き戻し。
- 初期版での他アーキテクチャ対応。

> 注意: perf のサンプルは「取得された時点の観測」であり、「画面更新時点の現在値」ではない。CPU を使っていないスレッドはサンプルが古くなり得る。

## 2. 既存実装の扱い

| 既存コード | 変更 |
| --- | --- |
| `src/snapshot/ptrace.rs` | 新方式が完成してから削除 |
| `src/snapshot/mod.rs` | `ProcessSnapshot` と停止取得処理を削除 |
| `src/snapshot/registers.rs` | `src/inspect/registers.rs` に移し、perf のレジスタマスクから変換するよう変更 |
| `src/snapshot/unwind_fp.rs` | `src/inspect/unwind_fp.rs` に移し、同一サンプルのスタックコピーからのみ unwind するよう変更 |
| `src/snapshot/disasm.rs` | `src/inspect/disasm.rs` に移し、サンプル IP を入力にできるよう変更 |
| `src/symbol/` | シンボル解決・PIE/ASLR 対応を再利用 |
| `src/process/` | 既存の `/proc`・`process_vm_readv()` を維持 |
| `src/state/mod.rs` | プロセス単位の sampler を共有する管理処理を追加 |
| `src/server/mod.rs` | `snapshot` を廃止し、既存 SSE に `samples` イベントを追加 |
| `src/web/app.ts`, `src/web/api-types.ts` | snapshot UI・型をサンプル表示へ置換 |

既存の `src/snapshot/unwind_fp.rs` は `walk()` とメモリ読み取りが分離されているため、`walk()` を生かして読み取り元をサンプル内のスタックコピーに差し替える。

## 3. 新しい構成

```text
/proc ────────────────> 既存のプロセス定期観測 ──────┐
process_vm_readv() ──> 既存のメモリ読み取り ────────┤
eBPF ────────────────> 既存のシステム活動収集 ──────┤
                                                    ├── Axum / SSE ──> UI
perf_event_open() ──> ring buffer ──> PerfWorker ──┤
                                       │            │
                                       └─ Samples ──┘
                                          │
                               Symbolizer / disassembler
```

新規モジュールの例:

```text
src/perf/
  mod.rs          # 公開 API・設定
  event.rs        # perf_event_open / perf_event_attr / FD 所有
  ring.rs         # mmap / head-tail / レコード切り出し
  decode.rs       # SAMPLE / LOST / EXIT などの解析
  worker.rs       # epoll・TID 増減・FD のライフサイクル
  store.rs        # 最新サンプル・履歴・欠落統計
src/inspect/
  mod.rs
  registers.rs
  unwind_fp.rs
  disasm.rs
```

サンプリングの所有者は `AppState` 内の `PerfManager` とする。`{pid, start_time_ticks}` をキーに sampler を共有し、詳細画面の SSE 購読数がゼロになったら FD と mmap を解放する。既存の SSE ごとの CPU/RSS 観測履歴は変更しなくてよい。

## 4. perf の初期設定

- `type = PERF_TYPE_SOFTWARE`, `config = PERF_COUNT_SW_CPU_CLOCK`
- `pid = tid`, `cpu = -1`, `group_fd = -1`, `flags = PERF_FLAG_FD_CLOEXEC`
- `freq = 1`, `sample_freq = 49`（設定可能、初期案として 1–199 Hz）
- `exclude_kernel = 1`, `exclude_hv = 1`, `inherit = 0`
- 第1段階: `PERF_SAMPLE_IP | PERF_SAMPLE_TID | PERF_SAMPLE_TIME | PERF_SAMPLE_CPU`
- 第2段階: `PERF_SAMPLE_REGS_USER` と x86-64 汎用レジスタ用 `sample_regs_user` マスクを追加
- 第3段階: `PERF_SAMPLE_CALLCHAIN` を追加。`PERF_SAMPLE_STACK_USER` は必要なスレッドに限定してオプションで有効化する。
- サンプル時刻は対応カーネルで `use_clockid = 1`, `clockid = CLOCK_MONOTONIC` とし、内部時刻を統一する。非対応なら明示した代替時刻方式を採用する。

初期値は各 TID に metadata 1 ページ + data 8 ページのリングバッファを割り当て、1 プロセス当たりのサンプリング TID 数は初期値 128 とする。どちらも測定前の暫定値。FD・locked memory の上限、`perf_event_mlock_kb`、`RLIMIT_NOFILE` によってはさらに制限し、全プロセスの観測まで失敗させずに部分収集と警告を返す。

`PERF_SAMPLE_STACK_USER` による大きいスタックコピーを全スレッド × 高周波数で常時有効にしない。まずユーザーコールチェーンを試し、フレームポインタの省略に対応したい場合だけ、選択スレッドのスタックコピーを有効にする。

## 5. スレッドとリングバッファの管理

1. 対象の `{pid, start_time_ticks}` を検証する。
2. `/proc/<pid>/task` から TID を列挙し、TID ごとに `perf_event_open()` する。TID 自身の start time も記録する。
3. 各 FD を `mmap` し、単一の専用ワーカーの `epoll` でまとめて処理する。HTTP の async タスク内でリングバッファを直接読む構成にはしない。
4. metadata の `data_head` を取得順序を守って読み、レコードをコピーまたは解析し、消費後の `data_tail` を公開順序を守って更新する。リング末尾をまたぐレコード、可変長フィールド、不正なサイズを考慮する。
5. `PERF_RECORD_SAMPLE` をデコードし、`PERF_RECORD_LOST` は欠落件数に加算する。`THROTTLE`・`UNTHROTTLE`・`EXIT` 等も必要な範囲で扱う。
6. 約1秒ごとに TID 集合を再走査し、新規 TID を登録、消滅 TID の FD を閉じる。短命スレッドの取りこぼしは初期版では許容する。
7. 対象の PID 再利用、`exec`、対象終了、購読ゼロ、サーバー終了で安全に収集を終了する。

`perf` が記録した時刻と、`/proc` から別途読んだ状態の時刻は一致しない。サンプルとマップを結びつける際は、その不整合を許容し、JIT・アンマップなどで解決不能なら生アドレスを残す。

## 6. サンプル型と保持方針

Rust 側の概念上の型:

```rust
struct ThreadSample {
    tid: i32,
    sampled_at_mono_ns: u64,
    cpu: u32,
    ip: u64,
    registers: Option<Vec<Register>>,
    call_stack: Vec<StackFrame>,
    unwind_stop: Option<String>,
    quality: SampleQuality,
}

enum SampleQuality {
    IpOnly,
    Registers,
    Callchain,
    StackCopy,
}
```

実際の公開型では `sampled_at_mono_ns` を **10進文字列**、アドレスを既存 API と同じ16進文字列として扱い、JavaScript の整数精度問題を避ける。公開応答には `sample_age_ms` をサーバー側で計算して含める。

各プロセスの `PerfStore` は、TID ごとの最新サンプル、直近60秒の軽量タイムライン、累計欠落数を保持する。生のスタックコピーは API 配信後まで長期保存しない。全体の保存量に上限（初期案: 16 MiB/プロセス）を置く。過負荷時は古い履歴を捨て、最新サンプルと欠落情報を優先する。

## 7. レジスタ・コールスタック・逆アセンブル

### レジスタ

`PERF_SAMPLE_REGS_USER` の ABI とマスクに従って x86-64 レジスタを復元する。現在の `registers::classify()` に渡し、既存のメモリマップ分類を再利用する。取得できないレジスタは 0 と偽装せず、欠損として扱う。`sample.ip` と `regs.RIP` が一致する保証も置かない。

### コールスタック

初期版は `PERF_SAMPLE_CALLCHAIN` のユーザーフレームを優先し、既存の `src/symbol/` でシンボル化する。カーネル側の収集可否、フレームポインタ省略、スタック深さ制限によって部分的な結果になり得る。必要になったら `PERF_SAMPLE_STACK_USER` と同一サンプルの `RSP/RBP` を使い、コピー済みバイト列だけで `unwind_fp::walk()` を実行する。サンプル後の `process_vm_readv()` でスタックを補完して「同一時点のスタック」として扱わない。DWARF unwind は別 Issue とする。

### 逆アセンブル

選択中スレッドの最新サンプル IP を起点に、必要時だけ `process_vm_readv()` で命令バイトを読み、既存の `iced-x86` を再利用してデコードする。読み取りはサンプル取得後なので、特に JIT や自己書き換えコードではサンプル時の命令列と異なり得る。UI に「サンプル時の IP / 命令バイトは後から読み取り」と明示する。既存の最大256バイト・最大32命令を維持する。

## 8. API と UI

### API

- 維持: `GET /api/processes`、`observation`、`threads`、`maps`、`memory`、`environment`、`auxv`、`fds`、`signals`、既存の `GET /api/processes/events`。
- 拡張: `GET /api/processes/events` が従来の `observation` に加えて、最新サンプル・欠落統計をまとめた `samples` イベントを最大1秒ごとに送る。接続直後にも現状を配信する。
- 追加: `GET /api/processes/disassembly?pid=...&start_time_ticks=...&address=...`。選択したサンプル IP だけを必要時にデコードする。従来のメモリ読み取り上限と識別子検証を守る。
- 廃止: `POST /api/processes/snapshot`。移行完了時にルート、型、テストを同じ変更で削除する。

`samples` の概念的 JSON:

```json
{
  "process_id": {"pid": 1234, "start_time_ticks": 5678},
  "status": "active",
  "configured_hz": 49,
  "lost_total": 0,
  "thread_limit_reached": false,
  "threads": [
    {
      "tid": 1234,
      "sampled_at_mono_ns": "845123456789",
      "sample_age_ms": 18,
      "ip": "0x00007f1234567890",
      "cpu": 3,
      "registers": [],
      "call_stack": [],
      "quality": "ip_only"
    }
  ]
}
```

`status` は `active | unavailable | partial | stopped` を想定し、`unavailable` では理由と推奨確認項目を返す。`registers` や `call_stack` が取得できない場合は、その理由を明示する。既存 SSE の送信負荷を抑えるため、49 Hz の生レコードをすべてブラウザに送らず、1秒に1回まとめて送る。

### UI

- スナップショット取得ボタンと1秒おきの自動 snapshot モードを削除し、詳細画面でサンプリングを自動開始する。
- スレッド一覧に `latest IP`、最後のサンプルからの経過時間、サンプリング状態を追加する。
- 最新のレジスタ・コールスタック・逆アセンブルを表示し、どのサンプルに由来するかを示す。
- スリープ中スレッドは `/proc` の `Sleeping` と、perf の `stale / no sample` を別々に表示する。**未サンプリングを停止・異常・ゼロ使用率と断定しない。**
- 履歴上のサンプルを選択できるようにする。Freeze はブラウザの表示を固定するだけで、対象プロセスは停止しない。
- 権限不足やサンプル欠落が生じても、通常の `/proc` パネルは引き続き利用できる。

## 9. 実装順序と完了条件

### Phase 0: 単独 PoC

- [x] 既存コードに統合する前に、CPU-bound なテストプロセス1スレッドから `IP/TID/TIME/CPU` を取得する。
- [x] `perf_event_paranoid` と CAP_PERFMON による利用可否を確認する。
- [x] `PERF_SAMPLE_REGS_USER` とユーザーコールチェーンが対象環境で取れるかを確認する。
- [x] システムの perf FD・リングバッファの利用可否と使用量、設定済み上限を確認する。（FD/locked-memory の限界までの枯渇試験は未実施）

### Phase 1: 最小の PerfWorker

- [x] `src/perf/{event,ring,decode,worker}.rs` を追加する。
- [x] 1 TID の ring から IP、TID、時刻、CPU を取得する。
- [x] wrap-around、可変長レコード、不正レコード、LOST を unit test する。
- [x] `/proc/<pid>/task` の再走査、TID 追加・削除、PID 再利用・対象終了を処理する。

### Phase 2: 既存 SSE への統合

- [x] `PerfManager` を `AppState` に追加し、プロセス単位で共有する。
- [x] `PerfStore` に最新サンプル・上限付き履歴・欠落統計を保存する。
- [x] 既存 `/api/processes/events` に `samples` を追加する。
- [x] 権限不足・資源不足時に `/proc` 観測を維持する。

### Phase 3: 情報の充実

- [x] `REGS_USER` の x86-64 マスクと ABI を解析して既存レジスタ分類へ接続する。
- [x] `CALLCHAIN` をシンボル化し、部分取得・失敗理由を返す。
- [ ] 必要なスレッドに `STACK_USER` を有効化し、コピー済みスタックで unwind する。（今回の範囲外・後続対応）
- [x] サンプル IP から必要時に逆アセンブルする API を追加する。

### Phase 4: UI と旧方式の削除

- [x] 既存 snapshot UI と自動 snapshot を新しいライブ表示に置換する。
- [x] `src/snapshot/ptrace.rs`、`ProcessSnapshot`、snapshot API と古いテストを削除する。
- [x] `rg 'ptrace|snapshot' src tests docs` で残る参照を確認する（一般説明の snapshot という語は必要なら残す）。
- [x] `docs/DEVELOPMENT.md`、README、API 型、ブラウザテストを更新する。

## 10. テスト計画

**権限不要の CI:** サンプルバイナリデータを使い、ring buffer の wrap-around、未知レコード、壊れた長さ、`REGS_USER` の ABI 別解析、欠落数、保持上限、PID/TID identity、SSE 初回・増分・切断、UI の stale/no sample 表示を検証する。既存の `tests/auto-snapshot.mjs` などは新方式のテストに置き換える。

**perf 利用可能な Linux 実機での結合テスト:** CPU-bound、`futex` 待機、複数スレッド、短命スレッド、終了直後、JIT/動的 `mmap`、リングバッファあふれ、複数タブ同時閲覧を確認する。perf が使えない共有 CI ではこれらを黙って成功扱いにせず、専用ジョブに分離する。

**負荷試験:** 1/8/64 スレッドについてサンプリング 19/49/99 Hz、コールチェーンあり・なしで、対象 CPU 使用率、procinsh 自身の CPU/RSS、FD 数、欠落数、SSE 送信量を測る。スリープ中心のスレッドでサンプルが増えないことも確認する。

## 11. リリース基準

- [x] 1スレッドと複数スレッドで非停止の IP/TID/TIME/CPU サンプリングが動作する。
- [x] 対応環境で汎用レジスタとユーザーコールチェーンが取得・表示できる。
- [x] スレッド増減、PID 再利用、終了、リングバッファ overflow を扱える。
- [x] 権限不足時も `/proc` と `process_vm_readv()` の既存 API は、各々の権限が許す範囲で動作する。
- [x] `samples` の鮮度と欠落を UI が区別する。
- [x] 複数 SSE 接続で sampler が重複せず、最後の接続終了後に FD と mmap が解放される。
- [x] 残存する `ptrace` システムコールがなく、snapshot API に依存する UI・テスト・ドキュメントがない。
- [x] サンプリングオーバーヘッドと既知の制約を実測して文書化した。

## 12. 参考資料

- [procinsh Issue #7](https://github.com/akawashiro/procinsh/issues/7)
- [現行 snapshot 実装](https://github.com/akawashiro/procinsh/tree/main/src/snapshot)
- [perf_event_open(2)](https://man7.org/linux/man-pages/man2/perf_event_open.2.html)
- [Linux kernel: Perf events and tool security](https://docs.kernel.org/admin-guide/perf-security.html)

## 13. 実装・検証記録（2026-09-26）

実装は `src/perf/` と `src/inspect/`。旧 `src/snapshot/` と停止取得 API/UI を削除した。SSE の初回・毎秒配信、PID/TID識別、共有と最終購読での解放、終了、exec、未知／壊れたレコード、LOST、ABI_NONE/32/64、ユーザー CALLCHAIN を扱う。権限拒否は子サーバーに perf のみを拒否する seccomp を適用して検証する。

実機では CPU-bound の IP/18レジスタ/CALLCHAIN、PIE/non-PIE/デバッグ情報なしのシンボル解決、sleep、スレッド増減、exec、動的 mmap、共有・解放を確認した。意図的にリングを読み止めるテストで LOST も確認した。ブラウザではライブ表示・Freeze・履歴・stale/no sample・取得不可・古い対象の通知破棄・逆アセンブル・既存詳細パネルを検証した。専用実機テストは通常 CI の ignored と明示し、手動の `Perf live tests` ジョブでは利用不可を失敗とする。

### 負荷測定

環境：Linux 6.18.33.2-microsoft-standard-WSL2、x86-64、12 logical CPUs、`perf_event_paranoid=2`、`perf_event_mlock_kb=516`、実行シェルの FD 上限10240・locked-memory上限65536 KiB。debug ビルド、ウォームアップ1.5秒＋測定4秒、各条件1回。各 workload に待機するリーダー1本を加え、表の数だけ CPU-bound worker を作成した。測定コマンド：`python3 tests/perf-bench.py > target/perf-bench.jsonl`。

全18条件で LOST=0、不正レコード=0、全 CPU-bound worker のサンプルを取得した。CPU % は1コア=100%。SSE量は `observation` と `samples` の合計で、短い測定窓の履歴量に依存する。

| Workers | Hz | CALLCHAIN | Server CPU % | RSS MiB | FD | SSE KiB/s | 対象処理量のbaseline比 % |
|---:|---:|:---:|---:|---:|---:|---:|---:|
| 1 | 19 | 無 | 1.5 | 14.1 | 14 | 19.2 | -1.3 |
| 1 | 19 | 有 | 2.7 | 15.1 | 14 | 19.8 | -56.1 |
| 1 | 49 | 無 | 2.5 | 14.5 | 14 | 30.8 | -21.4 |
| 1 | 49 | 有 | 2.2 | 15.4 | 14 | 31.4 | -14.9 |
| 1 | 99 | 無 | 3.0 | 14.3 | 14 | 50.0 | -12.8 |
| 1 | 99 | 有 | 3.5 | 15.6 | 14 | 50.6 | -10.1 |
| 8 | 19 | 無 | 8.5 | 15.0 | 21 | 91.3 | -5.1 |
| 8 | 19 | 有 | 8.7 | 16.0 | 21 | 96.7 | +0.7 |
| 8 | 49 | 無 | 13.5 | 15.6 | 21 | 184.5 | -2.3 |
| 8 | 49 | 有 | 12.7 | 16.2 | 21 | 188.1 | +4.8 |
| 8 | 99 | 無 | 17.2 | 16.2 | 21 | 338.5 | +9.7 |
| 8 | 99 | 有 | 18.2 | 16.9 | 21 | 342.7 | +6.4 |
| 64 | 19 | 無 | 16.9 | 16.1 | 77 | 236.7 | +10.2 |
| 64 | 19 | 有 | 20.9 | 18.0 | 77 | 299.5 | +9.8 |
| 64 | 49 | 無 | 21.9 | 16.7 | 77 | 392.0 | +9.6 |
| 64 | 49 | 有 | 27.4 | 17.9 | 78 | 313.3 | -1.5 |
| 64 | 99 | 無 | 28.5 | 17.3 | 77 | 337.7 | +6.2 |
| 64 | 99 | 有 | 26.6 | 18.1 | 77 | 250.1 | +7.4 |

基準測定は各 worker 数でサーバー接続前に4秒間行った。対象処理量は共有カウンターの反復/秒であり、scheduler・CPU周波数・WSLホスト負荷によるノイズを含む。改善方向の値も出ているため、これを厳密なサンプリングオーバーヘッドの推定値にはしない。長時間・反復測定・releaseビルドでの評価は継続課題。履歴は容量または60秒で打ち切り、それまで SSE量は増加する。リング容量はmetadataを含め1 TIDあたり9ページ、128 TIDで1152ページ（4 KiBページなら4.5 MiB）で、16 MiBの履歴予算とは別。

### 既知の制約・後続課題

- STACK_USER とコピー済みスタックの巻き戻し、DWARF unwind は未実装。
- 再走査間隔より短いスレッド、フレームポインタ省略、JITやマップ変更による解決不能を許容する。
- 非対応の MONOTONIC/remove_on_exec を別方式で補わず `unavailable` とする。
- シンボル解決と `/proc` マップはサンプルと同時刻ではない。逆アセンブルは後から読み取った命令列。
- 本測定は短時間の debug ビルド。長時間の安定性や他カーネルでの測定結果を保証しない。

### 最終回帰チェック（2026-09-27）

- `cargo build --locked --all-targets`、`cargo fmt --all --check`、`cargo clippy --all-targets --locked -- -D warnings`：成功。
- `npm run typecheck`、`npm run build:web`：成功。
- `cargo test --locked`：43件成功、perf専用6件は明示的にignored。
- `cargo test --locked --test perf_live -- --ignored --test-threads=1`：5件成功。129 TIDの対象で128 TIDの部分収集と通常観測の継続も確認。
- `cargo test --locked --lib live_ring_overflow -- --ignored`：成功。
- `node tests/browser.mjs`：成功。Chromeはローカルの公式配布バイナリを使用。
- `node tests/space-model.mjs`、`python3 tests/logging-checks.py`、`python3 tests/perf-unavailable.py`：成功。

process_vm_readv とローカルHTTPを拒否する実行サンドボックスでは該当テストが失敗するため、その制限外でローカルfixtureに対して実行した。BPF実センサーはこの環境で権限不足のため、今回の実機確認の対象外。既存SPACEの構造・接続管理・モデルの回帰テストは成功した。FD/locked-memoryを限界まで枯渇させる試験と長時間の負荷試験は未実施。
