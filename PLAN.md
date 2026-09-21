# procinsh 実装計画

## 1. 概要

`procinsh` は、Linux 上で **1つのプロセスを詳細に観察するための Web ベースのプロセスインスペクタ**である。

Rust で実装し、起動するとローカル HTTP サーバーを立ち上げる。

通常は、

```bash
procinsh
```

だけで起動し、ブラウザ上に現在動作しているプロセス一覧を表示する。

```text
http://127.0.0.1:8080
```

ユーザーは一覧から対象プロセスを選択し、そのプロセスについて詳細な監視画面を開く。

必要であれば PID を直接指定して、プロセス選択画面をスキップできる。

```bash
procinsh --pid 12345
procinsh --pid 12345 --interval 1s
```

初期ターゲットは以下とする。

* OS: Linux
* アーキテクチャ: x86-64 のみ
* 実装言語: Rust
* UI: HTML / CSS / JavaScript
* 更新間隔: ユーザーが指定可能
* 一度に詳細監視する対象: 1プロセス
* 対象プロセスの全スレッドを観測可能

---

# 2. 基本的な UX

## 2.1 通常起動

```bash
procinsh
```

を実行すると、

```text
procinsh is running:
http://127.0.0.1:8080
```

と表示する。

ブラウザを開くと、まずプロセス一覧を表示する。

```text
┌────────────────────────────────────────────────────────────────────┐
│ procinsh                                      Process Explorer │
├────────────────────────────────────────────────────────────────────┤
│ Search: [ firefox________________________ ]                        │
│                                                                    │
│ PID      USER      CPU    RSS      NAME             COMMAND        │
│ ───────────────────────────────────────────────────────────────── │
│ 8123     akira     82%    1.2G     target           ./target ...   │
│ 19231    akira     23%    812M     firefox          /usr/lib/...   │
│ 21341    akira      4%    145M     code             /usr/bin/...   │
│ 1        root       0%     14M     systemd          /sbin/init     │
│ ...                                                                │
│                                                                    │
│                         [ Refresh ]                                │
└────────────────────────────────────────────────────────────────────┘
```

対象プロセスをクリックすると詳細画面へ遷移する。

---

## 2.2 PID を直接指定

```bash
procinsh --pid 8123
```

の場合、プロセス一覧をスキップし、その PID の詳細画面を直接表示する。

---

## 2.3 将来的な直接起動

将来的には以下もサポートする。

```bash
procinsh -- ./target arg1 arg2
```

この場合は `target` を child process として起動し、そのプロセスをそのまま監視対象にする。

---

# 3. 目標

1つのプロセスについて以下を確認できるようにする。

* CPU 使用率
* RSS / VMS
* page fault
* context switch
* I/O
* スレッド一覧
* CPU affinity
* scheduler
* priority
* memory map
* memory map ごとの RSS / PSS
* 任意アドレスのメモリ内容
* x86-64 register
* call stack
* ELF symbol
* debug 情報がある場合の source file / line
* CPU / memory 等の履歴
* 低オーバーヘッドな継続 sampling

特に以下を主要機能とする。

```text
register
    ↓
その値が指している memory mapping
    ↓
memory viewer
```

例えば、

```text
RIP  0x555555561287 → ./target [r-x] +0x1287
RSP  0x7fffffffdad0 → [stack] +0x1ad0
RDI  0x555555782140 → [heap] +0x2140
RAX  0x00000000002a → 42
```

のように表示する。

---

# 4. 初期バージョンで対象外とするもの

最初のバージョンでは以下を扱わない。

* ARM / AArch64
* macOS
* Windows
* remote host
* kernel stack
* eBPF
* GPU profiling
* container-aware process discovery
* register 書き換え
* memory 書き換え
* arbitrary optimized binary に対する完全な DWARF unwind

基本的に **read-only inspector** とする。

---

# 5. 画面構成

大きく2画面に分ける。

```text
Process List
     │
     │ select
     ▼
Process Inspector
```

---

# 6. Process List

起動直後の画面。

Linux の、

```text
/proc/[0-9]+
```

を列挙して process list を構築する。

表示する情報:

* PID
* process name
* executable
* command line
* UID / username
* state
* CPU 使用率
* RSS
* thread count

例:

```text
PID      USER       CPU     RSS       THREADS   NAME
8123     akira      82.1%   1.2 GiB   12        inference
18231    akira       8.2%   820 MiB   47        firefox
19232    root        1.1%    82 MiB    5        NetworkManager
```

以下をサポートする。

* process name による検索
* PID による検索
* command line による検索
* CPU 順 sort
* RSS 順 sort
* PID 順 sort

初期状態では、

```text
CPU usage descending
```

を有力候補とする。

---

# 7. Process Identity

PID だけを process identity として扱わない。

Linux では PID が再利用される可能性があるため、

```text
PID
+
/proc/PID/stat の starttime
```

を組み合わせる。

```rust
struct ProcessId {
    pid: i32,
    start_time_ticks: u64,
}
```

詳細画面表示中に対象プロセスが終了し、同じ PID が別プロセスに再利用されても、自動的に別プロセスへ接続してはいけない。

その場合は、

```text
Process exited
```

と表示する。

---

# 8. Process Inspector

対象プロセスを選択すると詳細画面を表示する。

概略:

```text
┌──────────────────────────────────────────────────────────────┐
│ ← Processes                                                 │
│ target                         PID 8123          ● running    │
│ ./target --model policy.onnx                                │
├──────────────────────────────────────────────────────────────┤
│ CPU 72% │ RSS 612M │ Threads 8 │ Faults 21/s │ Ctx 18/s    │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│ CPU history                                                  │
│                                                              │
├──────────────────────────────┬───────────────────────────────┤
│ Threads                      │ Registers                     │
│                              │                               │
│ 8123 main          51%       │ RIP ... → target::run       │
│ 8124 worker        20%       │ RSP ... → [stack]           │
│                              │ RDI ... → [heap]             │
├──────────────────────────────┼───────────────────────────────┤
│ Call Stack                   │ Memory                        │
│                              │                               │
│ #0 inference()               │ 0x... 48 89 e5 ...           │
│ #1 worker()                  │ ...                           │
│ #2 main()                    │                               │
├──────────────────────────────┴───────────────────────────────┤
│ Memory Maps                                                  │
│                                                              │
│ 555... r-x ./target                                         │
│ 7ff... r-x libc.so                                          │
└──────────────────────────────────────────────────────────────┘
```

左上には必ず、

```text
← Processes
```

を置き、いつでも別プロセスへ切り替えられるようにする。

---

# 9. データの3分類

内部では以下を明確に区別する。

1. `ProcessObservation`
2. `PerfSample`
3. `ProcessSnapshot`

---

## 9.1 ProcessObservation

対象を停止せずに取得する通常の観測値。

```rust
struct ProcessObservation {
    timestamp: SystemTime,
    process_id: ProcessId,

    cpu_percent: f64,

    rss_bytes: u64,
    vms_bytes: u64,

    minor_faults: u64,
    major_faults: u64,

    voluntary_context_switches: u64,
    nonvoluntary_context_switches: u64,

    io: IoStats,

    threads: Vec<ThreadObservation>,
}
```

主に `/proc` から取得する。

---

## 9.2 PerfSample

`perf_event_open()` によって非同期に取得する低侵襲な sample。

```rust
struct PerfSample {
    timestamp: u64,

    pid: u32,
    tid: u32,
    cpu: u32,

    ip: u64,

    registers: Registers,
    stack: Vec<u8>,
}
```

UI では、

```text
Latest register sample
34 ms ago
```

のように取得時刻を必ず表示する。

---

## 9.3 ProcessSnapshot

対象を停止して取得する整合性のある snapshot。

```rust
struct ProcessSnapshot {
    captured_at: SystemTime,

    process_id: ProcessId,

    threads: Vec<ThreadSnapshot>,
    maps: Vec<MemoryMap>,
}
```

```rust
struct ThreadSnapshot {
    tid: i32,
    registers: Registers,
    call_stack: Vec<StackFrame>,
}
```

---

# 10. 技術スタック

Backend:

* Rust
* Tokio
* Axum

候補 crate:

```toml
[dependencies]
anyhow = "1"
axum = "0.8"
clap = { version = "4", features = ["derive"] }
nix = { version = "0.31", features = ["ptrace", "process", "uio"] }
procfs = "0.18"
object = "0.40"
addr2line = "0.26"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
```

Frontend は初期段階では、

* HTML
* CSS
* Vanilla JavaScript
* SVG または Canvas

のみとする。

Frontend resource は、

```rust
include_str!()
include_bytes!()
```

で Rust binary に埋め込み、配布物を1ファイルにする。

---

# 11. ディレクトリ構成

```text
procinsh/
├── Cargo.toml
├── README.md
├── IMPLEMENTATION_PLAN.md
└── src/
    ├── main.rs
    │
    ├── process/
    │   ├── mod.rs
    │   ├── discovery.rs
    │   ├── procfs.rs
    │   ├── maps.rs
    │   ├── memory.rs
    │   └── threads.rs
    │
    ├── snapshot/
    │   ├── mod.rs
    │   ├── ptrace.rs
    │   ├── registers.rs
    │   └── unwind_fp.rs
    │
    ├── perf/
    │   ├── mod.rs
    │   ├── syscall.rs
    │   ├── event.rs
    │   ├── ring_buffer.rs
    │   └── sample.rs
    │
    ├── symbol/
    │   ├── mod.rs
    │   ├── elf.rs
    │   └── dwarf.rs
    │
    ├── state/
    │   ├── mod.rs
    │   └── history.rs
    │
    ├── server/
    │   ├── mod.rs
    │   ├── api.rs
    │   └── sse.rs
    │
    └── web/
        ├── index.html
        ├── app.js
        └── style.css
```

---

# 12. Phase 1: Process Discovery

まず、

```bash
procinsh
```

で現在の process list を取得できるようにする。

以下を列挙する。

```text
/proc/[0-9]+
```

各 process について、

```text
/proc/PID/stat
/proc/PID/status
/proc/PID/cmdline
/proc/PID/exe
```

等を読む。

内部型:

```rust
struct ProcessSummary {
    identity: ProcessId,

    name: String,
    command_line: Vec<String>,

    uid: u32,
    username: Option<String>,

    state: ProcessState,

    cpu_percent: Option<f64>,
    rss_bytes: u64,
    thread_count: u32,
}
```

permission error が発生しても一覧全体を失敗させない。

取得できない項目だけ、

```text
N/A
```

とする。

---

# 13. Phase 2: Process List Web UI

Axum を起動する。

最低限の API:

```text
GET /
GET /api/processes
```

`GET /api/processes` は process list を JSON として返す。

Process List では、

* search
* sort
* refresh
* select

を実装する。

Process をクリックすると、

```text
/process/8123
```

相当の詳細画面へ遷移する。

実装上は SPA にしてもよい。

---

# 14. Phase 3: 基本 Process Inspector

対象 PID に対して以下を読む。

```text
/proc/PID/stat
/proc/PID/status
/proc/PID/statm
/proc/PID/io
/proc/PID/task/
```

表示するもの:

* CPU
* RSS
* VMS
* minor faults
* major faults
* voluntary context switches
* nonvoluntary context switches
* read/write bytes
* thread count
* current CPU
* nice
* priority

rate 系の値は前回との差分から算出する。

```text
CPU                   37.2 %
Minor faults          421 /s
Major faults            0 /s
Voluntary ctx          81 /s
Nonvoluntary ctx       17 /s
Read                  1.2 MB/s
Write                 8.2 KB/s
```

---

# 15. Phase 4: Live 更新

リアルタイム更新には SSE を使用する。

```text
collector
   │
   │ N ms
   ▼
ProcessObservation
   │
   ▼
SSE
   │
   ▼
Browser
```

HTML 全体は reload しない。

CLI:

```bash
procinsh --interval 1s
procinsh --interval 250ms
```

最小 interval:

```text
100 ms
```

程度とする。

標準:

```text
1 sec
```

履歴は標準で直近60秒程度保持する。

---

# 16. Phase 5: Thread View

以下を列挙する。

```text
/proc/PID/task/*
```

表示:

```text
TID       Name       CPU%     CPU     State
────────────────────────────────────────────
8123      main       31.2     3       R
8124      worker0    21.7     2       S
8125      worker1    20.4     4       S
```

各 thread について、

* TID
* name
* CPU
* state
* CPU usage
* context switch
* scheduler
* priority
* affinity

を取得する。

thread を選択すると Register / Call Stack panel の対象を変更する。

---

# 17. Phase 6: Memory Map

読むもの:

```text
/proc/PID/maps
/proc/PID/smaps
/proc/PID/smaps_rollup
```

内部型:

```rust
struct MemoryMap {
    start: u64,
    end: u64,

    readable: bool,
    writable: bool,
    executable: bool,
    private: bool,

    file_offset: u64,

    pathname: Option<String>,

    rss_bytes: Option<u64>,
    pss_bytes: Option<u64>,
}
```

表示:

```text
Start             End               Perm   RSS       Object
────────────────────────────────────────────────────────────
555555554000      55555556a000      r-xp   88K       ./target
555555780000      5555559a0000      rw-p   2.1M      [heap]
7ffff7c00000      7ffff7de0000      r-xp   1.6M      libc.so.6
7ffffffde000      7ffffffff000      rw-p   132K      [stack]
```

mapping はクリック可能にする。

---

# 18. Phase 7: Memory Viewer

任意アドレスの読み出しには、

```text
process_vm_readv()
```

を使用する。

API:

```text
GET /api/target/memory?address=0x555555781000&length=256
```

標準取得:

```text
256 bytes
```

最大:

```text
64 KiB
```

程度とする。

表示:

```text
Address: 0x555555781000

555555781000   48 89 e5 48 83 ec 20 48 89 7d e8 48 8b ...
555555781010   ...

ASCII           H..H.. H.}.H...
```

mapping 全体を自動取得しない。

---

# 19. Phase 8: Register Snapshot

x86-64 register inspection を ptrace で実装する。

対象:

```text
RIP
RSP
RBP

RAX
RBX
RCX
RDX

RSI
RDI

R8
R9
R10
R11
R12
R13
R14
R15

RFLAGS
```

基本フロー:

```text
PTRACE_SEIZE
      ↓
PTRACE_INTERRUPT
      ↓
waitpid()
      ↓
PTRACE_GETREGS
      ↓
resume
```

対象プロセスを停止する時間は可能な限り短くする。

---

# 20. Register と Memory Map の対応

register value が mapping 内を指している場合、その mapping を表示する。

```text
RIP  0x555555561287 → ./target [r-x] +0x1287
RSP  0x7fffffffdad0 → [stack] +0x1ad0
RDI  0x555555782140 → [heap] +0x2140
RAX  0x00000000002a → 42
```

最低限、

* executable mapping
* stack
* heap
* shared library
* anonymous mapping
* integer

を分類する。

アドレスとして妥当な register はクリック可能にし、Memory Viewer に遷移させる。

---

# 21. Phase 9: Coherent Snapshot

ボタン:

```text
[ Coherent Snapshot を取得 ]
```

処理:

```text
thread を列挙

各 TID:
    PTRACE_SEIZE

各 TID:
    PTRACE_INTERRUPT

全 TID の停止を確認

registers
memory maps
stack memory
call stacks
を取得

全 thread を resume
```

snapshot 中の thread creation / exit を考慮する。

特に重要なのは、

**エラー発生時にも対象プロセスを停止したまま残さないこと。**

RAII を使用する。

```rust
let snapshot = SnapshotGuard::capture(pid)?;
```

`Drop` で停止済み thread の resume を試みる。

---

# 22. Phase 10: Frame Pointer Call Stack

最初の call stack は frame pointer ベースに限定する。

x86-64:

```text
RIP = 現在位置
RBP = 現在の frame

[RBP + 0] = previous RBP
[RBP + 8] = return address
```

これを繰り返す。

終了条件:

* RBP が stack mapping 外
* alignment が不正
* previous RBP <= current RBP
* memory read failure
* 最大 frame 数到達

最大:

```text
256 frames
```

推奨 build option:

C/C++:

```bash
-fno-omit-frame-pointer
```

Rust:

```bash
RUSTFLAGS="-C force-frame-pointers=yes"
```

---

# 23. Phase 11: Symbol Resolution

runtime address を、

```text
runtime address
      ↓
memory mapping
      ↓
ELF file
      ↓
ELF relative address
      ↓
symbol
```

として解決する。

`object` crate を使用する。

表示:

```text
#0 inference::run +0x47
#1 worker +0x82
#2 run_loop +0x32
#3 main +0x61
```

---

# 24. Phase 12: Source Location

`addr2line` / `gimli` を使って、

* function
* source filename
* line
* inline frame

を解決する。

```text
#0 inference::run()
   src/inference.rs:128

#1 worker()
   src/worker.rs:83

#2 main_loop()
   src/main.rs:210
```

ELF / DWARF parse 結果は cache する。

---

# 25. Phase 13: perf_event_open

ここから continuous sampling を導入する。

基本構成:

```text
perf_event_attr
perf_event_open()
mmap ring buffer
ioctl ENABLE/DISABLE
sample parser
```

標準 sampling frequency:

```text
99 Hz
```

CLI:

```bash
procinsh \
    --pid 8123 \
    --interval 1s \
    --sample-frequency 99
```

sampling frequency と UI interval は別設定にする。

---

# 26. perf Sample

最低限以下を取得する。

```text
PERF_SAMPLE_IP
PERF_SAMPLE_TID
PERF_SAMPLE_TIME
PERF_SAMPLE_CPU
PERF_SAMPLE_REGS_USER
PERF_SAMPLE_STACK_USER
```

必要に応じて、

```text
PERF_SAMPLE_CALLCHAIN
```

も利用する。

x86-64 の register を sample に含める。

sampled state は必ず timestamp とセットで扱う。

---

# 27. Live Call Stack

```text
sampled registers
+
sampled stack bytes
```

から unwind する。

sample 後に現在の target stack を `process_vm_readv()` で読み直してはいけない。

異なる時間の状態が混ざるためである。

表示:

```text
Latest Call Stack
sampled 21 ms ago

#0 policy::forward()
#1 inference::run()
#2 worker()
#3 run_loop()
#4 main()
```

---

# 28. Call Stack Aggregation

sample を ring buffer に保存する。

期間:

```text
1 sec
5 sec
10 sec
30 sec
```

集約:

```text
main
└── run_loop                         100%
    ├── inference                    72%
    │   ├── policy::forward          54%
    │   └── memcpy                   18%
    └── update_sensors               28%
```

後から flame graph を追加する。

---

# 29. DWARF Unwind

frame pointer unwind が安定した後に実装する。

入力:

```text
sampled registers
+
sampled stack
+
.eh_frame / .debug_frame
```

`gimli` を使用する。

これは初期リリースには含めない。

---

# 30. API 案

Process discovery:

```text
GET /api/processes
```

選択中 process:

```text
GET /api/target
```

Process stats:

```text
GET /api/target/process
```

Threads:

```text
GET /api/target/threads
```

Memory maps:

```text
GET /api/target/maps
```

Memory:

```text
GET /api/target/memory?address=0x1234&length=256
```

Latest sample:

```text
GET /api/target/sample
```

Snapshot:

```text
POST /api/target/snapshot
```

Live updates:

```text
GET /api/target/events
```

SSE を使用する。

---

# 31. セキュリティ

標準では、

```text
127.0.0.1
```

にのみ bind する。

外部公開には明示的に、

```bash
procinsh --listen 0.0.0.0:8080
```

を指定させる。

Memory Viewer は、

* password
* authentication token
* private key
* application secret

等を表示できてしまう可能性がある。

そのため remote access は危険な機能として扱う。

---

# 32. 権限エラー

以下は Linux の設定によって失敗する。

* ptrace
* `process_vm_readv`
* `perf_event_open`

単純な、

```text
EPERM
```

だけではなく、

```text
対象プロセスのレジスタを取得できませんでした。

考えられる原因:
- プロセスの所有者が異なる
- kernel.yama.ptrace_scope
- perf_event_paranoid
- CAP_SYS_PTRACE がない
```

等を表示する。

`sudo` を自動実行しない。

---

# 33. Process List の特殊ケース

Process 一覧取得中にも process は生成・終了する。

したがって、

```text
/proc/1234
```

を発見した直後に process が終了して、

```text
/proc/1234/stat
```

が存在しなくなることは正常動作として扱う。

以下は process enumeration 全体の失敗にしてはいけない。

```text
ENOENT
ESRCH
EACCES
EPERM
```

該当 process を skip するか、一部情報を `N/A` にする。

---

# 34. 性能

Process List では全 process の `/proc` を読むため、詳細情報を読み過ぎない。

一覧では、

```text
stat
status の一部
cmdline
```

程度に限定する。

`smaps` 等は選択された1プロセスについてのみ読む。

重要な原則:

```text
Process List
    → 軽い情報のみ

Selected Process
    → 詳細情報
```

これにより `procinsh` 自身の overhead を抑える。

---

# 35. テスト用 Target

```text
tests/targets/
```

以下を用意する。

* `busy_loop`
* `sleeping`
* `threads`
* `allocator`
* `recursive`
* `mmap_test`

`recursive` は、

```text
main
→ foo
→ bar
→ baz
→ sleep
```

となるようにする。

debug build:

```text
-g
-fno-omit-frame-pointer
```

で call stack のテストに使用する。

---

# 36. Milestone

## M0 — Skeleton

* Cargo project
* `procinsh` binary
* CLI
* HTTP server
* error handling

---

## M1 — Process Explorer

* `/proc` enumeration
* process identity
* CPU / RSS
* process search
* sort
* process selection

完了条件:

```bash
procinsh
```

でブラウザに process list が表示される。

---

## M2 — Basic Inspector

* CPU
* RSS / VMS
* faults
* context switches
* I/O
* threads
* SSE
* history

完了条件:

一覧から process をクリックすると live dashboard が表示される。

---

## M3 — Memory Inspector

* maps
* smaps
* memory map UI
* `process_vm_readv`
* hex viewer

---

## M4 — Register Inspector

* ptrace
* x86-64 registers
* register → mapping
* clickable pointer

---

## M5 — Call Stack

* coherent snapshot
* frame-pointer unwind
* ELF symbolization
* source location

ここを最初の実用リリースとする。

---

# 37. 最初のリリース

`v0.1.0` に含める。

* x86-64 Linux
* Process Explorer
* process 検索
* process 選択
* process 切り替え
* `--pid` 直接指定
* CPU / RSS / VMS
* page faults
* context switches
* I/O
* thread list
* memory maps
* memory viewer
* register snapshot
* register → memory map
* coherent snapshot
* frame-pointer call stack
* ELF symbol
* source location

continuous perf sampling は `v0.2.0` とする。

---

# 38. 推奨実装順序

```text
1. Cargo / CLI
2. Axum server
3. /proc process enumeration
4. ProcessId = PID + starttime
5. Process List API
6. Process List UI
7. search / sort
8. process selection
9. /proc detailed collector
10. SSE
11. basic process dashboard
12. thread view
13. maps parser
14. memory viewer
15. ptrace register capture
16. register address classification
17. coherent multi-thread snapshot
18. frame-pointer unwind
19. ELF symbolization
20. source-line resolution

---- v0.1.0 ----

21. perf_event_open wrapper
22. perf mmap ring buffer
23. register sampling
24. stack sampling
25. live call stack
26. sample aggregation
27. flame graph
28. DWARF unwind
```

---

# 39. 重要な設計判断

## 起動画面は Process Explorer とする

`procinsh` は1プロセス専用 inspector だが、PID を事前に調べなくても使えるようにする。

基本操作は、

```text
procinsh
    ↓
Process Explorer
    ↓
対象をクリック
    ↓
Process Inspector
```

とする。

---

## 詳細監視は常に1プロセス

Process Explorer に多数の process が表示されても、詳細な `/proc/smaps`、ptrace、perf 等を使う対象は1プロセスだけとする。

---

## PID 再利用を考慮する

process の identity は、

```text
PID + starttime
```

とする。

---

## ptrace を通常監視に使わない

通常監視:

```text
/proc
```

continuous profiling:

```text
perf_event_open
```

coherent snapshot:

```text
ptrace
```

と責務を分離する。

---

## sample と現在値を混同しない

必ず、

```text
Latest sample
21 ms ago
```

と表示する。

---

# 40. v0.1.0 Definition of Done

以下が動作すること。

```bash
$ procinsh

procinsh is running:
http://127.0.0.1:8080
```

ブラウザを開くと process list が表示される。

```text
PID      CPU      RSS       NAME
8123     81%      1.2G      target
9123     12%      800M      firefox
...
```

対象をクリックすると、

```text
target
PID 8123
```

の詳細画面へ移動する。

以下が live 表示される。

```text
CPU
RSS
VMS
page faults
context switches
I/O
threads
memory maps
```

thread を選択して、

```text
Coherent Snapshot を取得
```

を実行すると、

```text
registers
call stack
register → memory map
```

が表示される。

例えば、

```text
RIP → ./target
RSP → [stack]
RDI → [heap]
```

をクリックすると、そのアドレスの Memory Viewer が開く。

debug symbol と frame pointer が存在する target では、

```text
function
source file
line number
```

まで call stack を解決できる。

詳細画面から、

```text
← Processes
```

を押せば process list に戻り、別の process を選択できる。

snapshot 中にエラーが発生した場合でも対象 thread を停止したまま残さない。

この状態を `procinsh v0.1.0` の完成条件とする。
