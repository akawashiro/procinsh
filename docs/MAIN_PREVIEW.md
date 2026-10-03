# main の常設プレビュー

開発用 checkout と別の detached worktree で最新 main をビルドし、systemd の system service で常設します。待受は `0.0.0.0:9090` です。更新処理が終了するたび約10秒後に main を再確認します。ビルド中は旧版が動き続け、成功した場合だけ再起動します。

## 前提

同梱 unit はユーザー `akira`、開発用 checkout `/home/akira/ghq/github.com/akawashiro/procinsh`、専用領域 `/home/akira/.local/share/procinsh-preview` を使用します。専用領域内の worktree は更新処理が管理するため、手作業で編集しないでください。開発用 checkout のブランチとファイルは変更しませんが、専用 Git ref と worktree 登録を追加します。

[開発環境](DEVELOPMENT.md)のビルド依存に加え、`gh`、`curl`、`flock` が必要です。`akira` として `gh auth login` を済ませてください。更新処理は HTTPS と `gh auth git-credential` を使います。Cargo は `/home/akira/.cargo/bin`、Node.js 22 以降と npm は `/usr/local/bin` または `/usr/bin` から利用できる必要があります。別の場所にある場合は更新 unit の `PATH` を調整してください。

サービスは `User=akira` として起動し、systemd が `CAP_SYS_PTRACE`、`CAP_BPF`、`CAP_PERFMON` を付与します。ビルドは `akira` で行い、配置とサービス再起動を行う `ExecStartPost=+` の段階だけ root で実行します。この構成は当該ユーザーのビルド成果物を信頼する開発マシン向けです。

## 登録

以下はユーザーが実行するホスト設定です。リポジトリのルートで実行します。

```sh
mkdir -p /home/akira/.local/share/procinsh-preview
sudo mkdir -p /usr/local/libexec
sudo install -m 0755 scripts/main_preview.sh /usr/local/libexec/procinsh-main-preview
sudo install -m 0644 scripts/systemd/procinsh-preview.service \
  scripts/systemd/procinsh-preview-update.service \
  scripts/systemd/procinsh-preview-update.timer /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable procinsh-preview.service
sudo systemctl enable --now procinsh-preview-update.timer
sudo systemctl start procinsh-preview-update.service
```

初回ビルド成功後に常設サービスが起動します。以降は OS 再起動時にも起動します。unit と更新スクリプト自身は自動更新の対象ではありません。これらを変更したときは timer を停止して更新処理の終了を待ち、再インストールと `daemon-reload` を行ってから timer を再開してください。

外の端末から `http://開発マシンのLANまたはVPNアドレス:9090/` を開きます。

## 更新と運用

`origin/main` に相当する GitHub の main を専用 ref に取得し、`current.commit` と異なる場合だけ `npm ci`、`npm run build:web`、`cargo build --locked` を実行します。作業ブランチや未コミット変更は公開しません。timer は `OnUnitInactiveSec=10s`、`AccuracySec=1s` を使うため、長いビルド中に次の更新は重なりません。スクリプトもファイルロックで多重実行を防ぎます。

ビルド失敗時は稼働版を維持します。成功時は同じ専用領域内でバイナリを原子的に入れ替えて再起動し、約10秒を目安に loopback の HTTP 応答とサービス状態を確認します。起動失敗時は直前のバイナリと commit に戻します。初回起動が失敗した場合は候補を撤去してサービスを停止します。失敗した main は次回の更新で再試行します。更新時には短い切断が発生し、観測履歴がリセットされます。

```sh
# 稼働中の commit とログ
cat /home/akira/.local/share/procinsh-preview/current.commit
journalctl -u procinsh-preview.service -u procinsh-preview-update.service -f

# 次回を待たずに更新
sudo systemctl start procinsh-preview-update.service

# 常設と自動更新を停止（進行中の更新があれば先に終了を待つ）
sudo systemctl disable --now procinsh-preview-update.timer
sudo systemctl stop procinsh-preview-update.service
sudo systemctl disable --now procinsh-preview.service
```

外部接続、再起動後の起動、観測機能、更新と復元の実機確認はユーザーが行います。
