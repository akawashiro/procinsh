- github を使うときは gh コマンドを使って
- 機能を実装するときは最新の main ブランチからブランチを作って PR を作って
- インターフェース（re-export、型、関数シグネチャ、visibility）を変更した場合は、対応する module root の `//! # Interface` コメントの一覧・型・シグネチャ・定義リンクにも必ず反映すること。
  理由: 固定ツールチェーン Rust 1.98.1 の rustdoc では、非公開の祖先モジュールを持つ `pub(super)` / `pub(in ...)` などの restricted re-export が、façade 側の I/F として表示されない。`--document-private-items` や `#[doc(inline)]` でもこの構成では期待する表示にならないため、module root のコメントに型・シグネチャと定義元への intra-doc link を記載して補う。コメント内のシグネチャは自動同期されず、リンク検査でも不一致を検出できないため、インターフェース変更時に手動で更新する必要がある。
