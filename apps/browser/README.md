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
- 既知の制限(P1 で解消): 凍結は未実装(非表示のみ)、フォーム内容・スクロール位置は保存時に取得しない、
  この環境(AMD GPU)では GPU プロセスが再起動を繰り返すログが出る(描画はソフトウェアにフォールバック)。
