# browser (P0 プロトタイプ)

CEF(Views)上で動く最小のブラウザ。ロジックは `crates/shell-core`、ここは CEF とのつなぎ込みだけ。
ルートのワークスペースには含めない(CEF が必要なため `verify.ps1` の対象外)。

```
powershell -NoProfile -ExecutionPolicy Bypass -File apps/browser/run.ps1 [-Release] [-Url https://example.com/]
```

- プロファイル: `%LOCALAPPDATA%\RustBrowser`(`RUSTBROWSER_PROFILE` で変更)。`profile.db`(履歴・ブックマーク・セッション)、
  `policy.json`(任意。`crates/policy` の形式)、`cef-cache/`。
- 操作: ◀ ▶ ⟳、アドレスバー(URL / 検索語 + Enter)、☆/★(ブックマーク切替)、＋(新規タブ)、タブ名(切替)、×(閉じる)。
- 終了時と 30 秒ごとにセッションを保存。再起動で復元し、アクティブだったタブだけを実体化する(他は破棄状態)。
- サンドボックスは常に有効。CEF の `bootstrap.exe` を `rustbrowser.exe` として置き、`rustbrowser.dll` の `RunWinMain` を呼ばせる
  (`run.ps1` が配置する。直接 `cargo run` では起動できない)。
- 診断: プロファイル直下に空の `discard-now` ファイルを置くと、次の Tick(30 秒以内)で非表示のタブを破棄する。
- 既知の制限(P1 で解消): 凍結は未実装(非表示のみ)、フォーム内容・スクロール位置は保存時に取得しない、
  この環境(AMD GPU)では GPU プロセスが再起動を繰り返すログが出る(描画はソフトウェアにフォールバック)。

## 機能メモ
- **広告・トラッカーのブロック**: プロファイル直下の `blocklist.txt` を起動時に読む。Adblock Plus 形式の `||ドメイン^`(`$third-party`、
  例外 `@@||...`)と hosts 形式(`0.0.0.0 ドメイン`)に対応。パス付き・ワイルドカード・他のオプションの規則は無視する(誤って
  サイトを壊さないため)。上限 30 万規則。画像・スクリプト・iframe などのサブリソースも、アドレス欄・リンク遷移と同じく検査される。
  リストの自動更新は無い(アップデータの作業)。ファイルが無ければ何もブロックしない。
- **ダウンロード**: 「ダウンロード」フォルダへ保存。ファイル名はページの指定を信用せず整形し(パス区切り・予約名・末尾ドットの除去)、
  同名は `名前 (1).拡張子`。**実行形式・スクリプト・ショートカット等(exe/msi/bat/ps1/js/lnk など)は保存しない**。完了はタイトルに出す。
- **起動時の URL**: `--url=URL`、`--urls-file=PATH`(1 行 1 URL、最大 50 件)。通常の入力と同じ検査を通る。
- **配布物**: `apps/browser/package.ps1 [-Sign -CertThumbprint <sha1>] [-SmokeTest]` で `dist/RustBrowser-<version>-win-x64.zip` を作る
  (約 180MB)。署名は signtool(Windows SDK)で SHA-256。証明書はこのリポジトリに置かない。`-SmokeTest` は zip を別の場所へ展開して
  起動し、ウィンドウが出るまでを確認する。インストーラ(MSI / Inno Setup など)は未作成。
- **メモリの自動計測**: `bench/memory/run-cef-bench.ps1`(docs/roadmap-cef.md)。
- **パスワード管理**: ログイン送信時に確認バーで保存を尋ね、保存済みのサイトでは確認の後に自動入力する。パスワードは Windows の資格情報
  マネージャーに置き、DB には索引だけを置く。🔑 ボタンで一覧・削除(パスワードは表示しない)。詳細は docs/roadmap-cef.md。

