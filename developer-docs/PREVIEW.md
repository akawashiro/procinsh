# main・公開最新版の常設プレビュー

最新 main と、`cargo install procinsh --locked` でインストールできる crates.io の公開最新版を、systemd の user service で常設します。更新処理の終了から main は約10秒後、公開版は約1分後に再確認します。ビルド中は旧版が動き続け、成功した場合だけ再起動します。

| 対象 | 待受 | 専用領域 | 本体 service |
|---|---|---|---|
| main | `<Tailscale IPv4>:9090` | `~/procinsh-main-preview` | `procinsh-preview.service` |
| 公開最新版 | `<Tailscale IPv4>:9091` | `~/procinsh-release-preview` | `procinsh-release-preview.service` |

両版とも起動時に `tailscale ip -4` でこのホストの Tailscale IPv4 を取得し、その IP だけで待ち受けます。Tailscale の IP を取得できない場合は起動しません。LAN の IP や wildcard・loopback には待ち受けません。Tailscale 接続が起動時にまだ利用できない場合は、サービスの自動再起動で再試行します。

両方または必要な片方を登録できます。既存 main のサービス名と配置先は変更しません。

## 前提

ホストを Tailscale に接続し、ログインユーザーが `tailscale ip -4` で IPv4 を取得できることが前提です。`tailscale` は `/usr/local/bin`、`/usr/bin` または `/bin` から利用できる必要があります。それ以外の場所にある場合は本体・更新 unit の `PATH` を調整してください。

main 用 unit はログインユーザーのホームを基準に、開発用 checkout `~/ghq/github.com/akawashiro/procinsh`、専用領域 `~/procinsh-main-preview` を使用します。専用領域内の worktree は更新処理が管理するため、手作業で編集しないでください。開発用 checkout のブランチとファイルは変更しませんが、専用 Git ref と worktree 登録を追加します。

[開発環境](DEVELOPMENT.md)のビルド依存に加え、`gh`、`curl`、`flock` が必要です。利用するユーザーとして `gh auth login` を済ませてください。更新処理は HTTPS と `gh auth git-credential` を使います。Cargo は `~/.cargo/bin`、Node.js 22 以降と npm は `/usr/local/bin` または `/usr/bin` から利用できる必要があります。npm が PATH にない場合は `${NVM_DIR:-$HOME/.nvm}/nvm.sh` を読み込み、nvm の `default` alias を選択します。nvm を使う場合は `nvm alias default` が Node.js 22 以降を指すよう設定してください。それ以外の場所にある場合は更新 unit の `PATH` を調整してください。systemd は `.zshrc` などのシェル初期化ファイルを読み込みません。

公開版は `~/procinsh-release-preview/install` を Cargo の専用インストール先として使用します。通常の `~/.cargo/bin/procinsh` は変更しません。Rust とネイティブ・BPF のビルド依存、`curl`、`flock`、`cmp` が必要ですが、公開パッケージには生成済み JavaScript が含まれるため、Node.js・npm・GitHub 認証・開発用 checkout は不要です。Cargo は `~/.cargo/bin` から利用できる必要があります。公開版の更新は専用領域で実行するため、開発用 checkout の `rust-toolchain.toml` は適用されません。rustup の default toolchain は公開版をビルドできるバージョンに設定してください。

ビルド・配置・サービス管理はすべてログインユーザーで行います。ビルド後の候補バイナリに対して `sudo -n setcap cap_sys_ptrace,cap_bpf,cap_perfmon,cap_dac_read_search=ep` を実行し、観測用の権限を付与します。このコマンドを候補ファイル `~/procinsh-main-preview/candidate` と `~/procinsh-release-preview/candidate` に対してパスワード入力なしで実行できることが前提です。権限付与に失敗した場合は更新を中止し、稼働版を維持します。

## 登録

### sudoers の設定

自動更新を開始する前に、候補バイナリへの `setcap` を `NOPASSWD` で許可してください。`dev_run.sh` 用の設定が開発用バイナリのパスに限定されている場合は、常設版の候補パスを追加する必要があります。既存の設定はそのまま残します。

`command -v setcap` で実行ファイルのパスを確認し、`visudo` で設定を追加します。

```sh
command -v setcap
sudo visudo -f /etc/sudoers.d/procinsh
```

ユーザーが `akira`、`setcap` が `/usr/sbin/setcap` にある場合の設定例です。ユーザー名・ホーム・実行ファイルのパスは実際の環境に合わせ、絶対パスで記載してください。

```sudoers
akira ALL=(root) NOPASSWD: /usr/sbin/setcap cap_sys_ptrace\,cap_bpf\,cap_perfmon\,cap_dac_read_search\=ep /home/akira/procinsh-main-preview/candidate
akira ALL=(root) NOPASSWD: /usr/sbin/setcap cap_sys_ptrace\,cap_bpf\,cap_perfmon\,cap_dac_read_search\=ep /home/akira/procinsh-release-preview/candidate
```

sudoers では capability の区切りのカンマを `\,`、等号を `\=` としてエスケープします。このルールはコマンド・引数・対象パスを指定して許可します。[sudoers の説明](https://www.sudo.ws/docs/man/1.9.14/sudoers.man.pdf)を参照してください。保存後に構文を確認します。

```sh
sudo visudo -c
```

### main の user service の登録

以下はユーザーが実行するホスト設定です。リポジトリのルートで実行します。

```sh
mkdir -p "$HOME/procinsh-main-preview" "$HOME/.config/systemd/user"
install -m 0755 scripts/main_preview.sh scripts/preview_bind.sh "$HOME/procinsh-main-preview/"
install -m 0644 scripts/systemd/procinsh-preview.service \
  scripts/systemd/procinsh-preview-update.service \
  scripts/systemd/procinsh-preview-update.timer "$HOME/.config/systemd/user/"
mkdir -p "$HOME/.config/systemd/user/procinsh-preview.service.d"
cat > "$HOME/.config/systemd/user/procinsh-preview.service.d/logging.conf" <<'EOF'
[Service]
Environment=RUST_LOG=debug
EOF
systemctl --user daemon-reload
systemctl --user enable procinsh-preview.service
systemctl --user enable --now procinsh-preview-update.timer
systemctl --user start procinsh-preview-update.service
```

### 公開最新版の user service の登録

リポジトリのルートで実行します。既に main を登録している場合も、以下を追加できます。

```sh
mkdir -p "$HOME/procinsh-release-preview" "$HOME/.config/systemd/user"
install -m 0755 scripts/release_preview.sh scripts/preview_bind.sh "$HOME/procinsh-release-preview/"
install -m 0644 scripts/systemd/procinsh-release-preview.service \
  scripts/systemd/procinsh-release-preview-update.service \
  scripts/systemd/procinsh-release-preview-update.timer "$HOME/.config/systemd/user/"
mkdir -p "$HOME/.config/systemd/user/procinsh-release-preview.service.d"
cat > "$HOME/.config/systemd/user/procinsh-release-preview.service.d/logging.conf" <<'EOF'
[Service]
Environment=RUST_LOG=debug
EOF
systemctl --user daemon-reload
systemctl --user enable procinsh-release-preview.service
systemctl --user enable --now procinsh-release-preview-update.timer
systemctl --user start procinsh-release-preview-update.service
```

### 共通の運用設定

初回ビルド成功後に常設サービスが起動します。以降はユーザーマネージャー起動時に自動起動します。ログアウト後も稼働させ、OS 起動時からログインせず利用するには、一度 `loginctl enable-linger "$USER"` を実行してください。これはユーザーの linger 設定を有効にします。[systemd の説明](https://www.freedesktop.org/software/systemd/man/252/loginctl.html)を参照してください。unit と更新スクリプト自身は自動更新の対象ではありません。これらを変更したときは timer を停止して更新処理の終了を待ち、再インストールと `daemon-reload` を行ってから本体サービスを再起動し、timer を再開してください。既存の wildcard 待受から移行する場合も、`preview_bind.sh` の配置と本体サービスの再起動が必要です。再インストールと `daemon-reload` の後に、main は `systemctl --user restart procinsh-preview.service`、公開版は `systemctl --user restart procinsh-release-preview.service` を実行してください。

capability 一覧を変更した場合は、sudoers の許可する引数も新しい一覧に合わせてください。配置済みの更新スクリプトを再インストールするだけでは、既存バイナリの権限は変わりません。同じ commit や同じ公開版バイナリでは更新を省略するため、timer を停止して進行中の更新が終了した後、既存バイナリを `candidate` にコピーし、上記の `sudo -n setcap` をその候補に実行してから `procinsh` に置き換え、サービスを再起動してください。稼働 commit または version の記録は維持し、起動と権限を確認してから timer を再開します。置き換え前のバイナリは hard link で退避すると capability を保持したまま復元できます。未反映の候補が残っている場合は、候補と `candidate.commit`（公開版は `candidate.version`）を先に退避してください。

以下のサービス操作では、公開版の場合は `procinsh-preview` を `procinsh-release-preview` に読み替えてください。

本体のログは `RUST_LOG=debug` で journal に出力します。`journalctl --user -u procinsh-preview.service -f` で確認できます。この drop-in 設定は unit の再インストール後も維持されます。既に本体が稼働している場合は、`daemon-reload` 後に `systemctl --user restart procinsh-preview.service` を実行してログレベルを反映してください。

## 更新と運用

### main の更新

`origin/main` に相当する GitHub の main を専用 ref に取得し、`current.commit` と異なる場合だけ `npm ci`、`npm run build:web`、`cargo build --locked` を実行します。作業ブランチや未コミット変更は公開しません。timer の `OnUnitInactiveSec` は main が `10s`、公開版が `1min` で、両方とも `AccuracySec=1s` を使います。更新処理の終了を基準にするため、長いビルド中に次の更新は重なりません。スクリプトもファイルロックで多重実行を防ぎます。

### 公開最新版の更新

`cargo install procinsh --locked --registry crates-io --root "$HOME/procinsh-release-preview/install"` を実行し、Cargo の更新判定に従って公開最新版をインストールします。`--force` は指定しないため、同じ版は再ビルドしません。ビルドキャッシュは専用領域の `target` に保持します。毎回 crates.io の更新確認を行うため、ネットワーク接続が必要です。

インストール済みバイナリを稼働版と比較し、同一なら再起動しません。変更時だけ候補を配置して capability を付与し、反映します。稼働版のバージョンは `current.version` に記録します。インストール先と稼働版を分けているため、起動失敗後も Cargo のインストール記録に影響されず、次回更新で新版を再試行できます。公開版は一つのファイルロックでインストールから起動確認・復元まで多重実行を防ぎます。

### 更新失敗と復元

ビルド・インストール・権限付与失敗時は稼働版を維持します。成功時は同じ専用領域内でバイナリを原子的に入れ替えて再起動し、約10秒を目安に Tailscale IPv4 の HTTP 応答とサービス状態を確認します。起動失敗時は直前のバイナリと commit または version に戻します。初回起動が失敗した場合は候補を撤去してサービスを停止します。失敗した版は次回の更新で再試行します。更新時には短い切断が発生し、観測履歴がリセットされます。

```sh
# main: 稼働中の commit とログ
cat "$HOME/procinsh-main-preview/current.commit"
journalctl --user -u procinsh-preview.service -u procinsh-preview-update.service -f

# 次回を待たずに更新
systemctl --user start procinsh-preview-update.service

# 常設と自動更新を停止（進行中の更新があれば先に終了を待つ）
systemctl --user disable --now procinsh-preview-update.timer
systemctl --user stop procinsh-preview-update.service
systemctl --user disable --now procinsh-preview.service
```

```sh
# 公開最新版: 稼働中のバージョンとログ
cat "$HOME/procinsh-release-preview/current.version"
journalctl --user -u procinsh-release-preview.service -u procinsh-release-preview-update.service -f

# 次回を待たずに更新
systemctl --user start procinsh-release-preview-update.service

# 常設と自動更新を停止（進行中の更新があれば先に終了を待つ）
systemctl --user disable --now procinsh-release-preview-update.timer
systemctl --user stop procinsh-release-preview-update.service
systemctl --user disable --now procinsh-release-preview.service
```

登録後は次のコマンドで状態と HTTP 応答を確認してください。片方だけ登録した場合は対応するサービス・ポートだけ確認します。

```sh
systemctl --user status procinsh-preview.service procinsh-release-preview.service
preview_ip=$(tailscale ip -4)
curl --noproxy '*' --fail "http://$preview_ip:9090/"
curl --noproxy '*' --fail "http://$preview_ip:9091/"
```

Tailscale に接続した端末からは、このホストの Tailscale IPv4 の 9090・9091 に接続します。ログアウト・OS 再起動後も接続できること、観測機能が動くこと、main の新しい commit と公開版の新しいバージョンがそれぞれ反映されることを実機で確認してください。更新失敗時の復元確認は検証用ホストで行ってください。

外部接続、再起動後の起動、観測機能、更新と復元の実機確認はユーザーが行います。
