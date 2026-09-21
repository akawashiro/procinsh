# 実装状況

仕様は [PLAN.md](PLAN.md)、実行手順と制約は [README.md](README.md) を参照。

- [x] M0: Cargo / CLI / Axum / 埋め込み Web リソース / エラーハンドリング
- [x] M1: `/proc` discovery / PID + starttime / CPU / RSS / 検索 / sort / 選択・切り替え
- [x] M2: 非停止の詳細観測 / 全スレッド / 差分 rate / SSE / 60秒履歴
- [x] M3: maps / smaps / rollup / process_vm_readv / hex・ASCII / partial read
- [x] M4: x86-64 registers / register → mapping / Memory Viewer へのリンク
- [x] M5: coherent multi-thread snapshot / RAII cleanup / frame-pointer unwind / ELF・DWARF cache / source line・inline frame
- [x] fixture と unit / integration / browser tests
- [ ] v0.2.0: perf_event_open / ring buffer / sampled registers・stack / live call stack / aggregation
- [ ] 将来: flame graph / DWARF unwind / child process 直接起動

通常の観測値と ProcessSnapshot は別の型・取得経路で管理する。PerfSample は v0.2.0 で追加する。snapshot のマップと現在メモリの読み取り時刻を UI で区別する。
