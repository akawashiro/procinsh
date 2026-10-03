# main の常設プレビュー

開発用 checkout と別の detached worktree で最新 main をビルドし、systemd の user service で常設します。待受は `0.0.0.0:9090` です。更新処理が終了するたび約10秒後に main を再確認します。ビルド中は旧版が動き続け、成功した場合だけ再起動します。

## 前提

同梱 unit はログインユーザーのホームを基準に、開発用 checkout `~/ghq/github.com/akawashiro/procinsh`、専用領域 `~/.local/share/procinsh-preview` を使用します。専用領域内の worktree は更新処理が管理するため、手作業で編集しないでください。開発用 checkout のブランチとファイルは変更しませんが、専用 Git ref と worktree 登録を追加します。

[開発環境](DEVELOPMENT.md)のビルド依存に加え、`gh`、`curl`、`flock` が必要です。利用するユーザーとして `gh auth login` を済ませてください。更新処理は HTTPS と `gh auth git-credential` を使います。Cargo は `~/.cargo/bin`、Node.js 22 以降と npm は `/usr/local/bin` または `/usr/bin` から利用できる必要があります。別の場所にある場合は更新 unit の `PATH` を調整してください。

ビルド・配置・サービス管理はすべてログインユーザーで行います。ビルド後の候補バイナリに対して `sudo -n setcap cap_sys_ptrace,cap_bpf,cap_perfmon=ep` を実行し、観測用の権限を付与します。このコマンドを候補ファイル `~/.local/share/procinsh-preview/candidate` に対してパスワード入力なしで実行できることが前提です。権限付与に失敗した場合は更新を中止し、稼働版を維持します。

## 登録

以下はユーザーが実行するホスト設定です。リポジトリのルートで実行します。

```sh
mkdir -p "$HOME/.local/share/procinsh-preview" "$HOME/.config/systemd/user"
install -m 0755 scripts/main_preview.sh "$HOME/.local/share/procinsh-preview/main_preview.sh"
install -m 0644 scripts/systemd/procinsh-preview.service \
  scripts/systemd/procinsh-preview-update.service \
  scripts/systemd/procinsh-preview-update.timer "$HOME/.config/systemd/user/"
systemctl --user daemon-reload
systemctl --user enable procinsh-preview.service
systemctl --user enable --now procinsh-preview-update.timer
systemctl --user start procinsh-preview-update.service
```

初回ビルド成功後に常設サービスが起動します。以降はユーザーマネージャー起動時に自動起動します。ログアウト後も稼働させ、OS 起動時からログインせず利用するには、一度 `loginctl enable-linger "$USER"` を実行してください。これはユーザーの linger 設定を有効にします。[systemd の説明](https://www.freedesktop.org/software/systemd/man/252/loginctl.html)を参照してください。unit と更新スクリプト自身は自動更新の対象ではありません。これらを変更したときは timer を停止して更新処理の終了を待ち、再インストールと `daemon-reload` を行ってから timer を再開してください。

外の端末から `http://開発マシンのLANまたはVPNアドレス:9090/` を開きます。

## 更新と運用

`origin/main` に相当する GitHub の main を専用 ref に取得し、`current.commit` と異なる場合だけ `npm ci`、`npm run build:web`、`cargo build --locked` を実行します。作業ブランチや未コミット変更は公開しません。timer は `OnUnitInactiveSec=10s`、`AccuracySec=1s` を使うため、長いビルド中に次の更新は重なりません。スクリプトもファイルロックで多重実行を防ぎます。

ビルド失敗時は稼働版を維持します。成功時は同じ専用領域内でバイナリを原子的に入れ替えて再起動し、約10秒を目安に loopback の HTTP 応答とサービス状態を確認します。起動失敗時は直前のバイナリと commit に戻します。初回起動が失敗した場合は候補を撤去してサービスを停止します。失敗した main は次回の更新で再試行します。更新時には短い切断が発生し、観測履歴がリセットされます。

```sh
# 稼働中の commit とログ
cat "$HOME/.local/share/procinsh-preview/current.commit"
journalctl --user -u procinsh-preview.service -u procinsh-preview-update.service -f

# 次回を待たずに更新
systemctl --user start procinsh-preview-update.service

# 常設と自動更新を停止（進行中の更新があれば先に終了を待つ）
systemctl --user disable --now procinsh-preview-update.timer
systemctl --user stop procinsh-preview-update.service
systemctl --user disable --now procinsh-preview.service
```

外部接続、再起動後の起動、観測機能、更新と復元の実機確認はユーザーが行います。
