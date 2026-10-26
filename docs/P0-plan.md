# P0 プロトタイプ 実行計画(CEF 経由)

目的(設計書 §10 フェーズ0 / ゲート G1 の前段): CEF 上で起動し、タブ・履歴・ブックマーク・
セッション復元が動く最小のブラウザを作る。Chromium 本体のビルドは不要(CEF のビルド済み配布物を使う)。

## 方針
- 判断ロジックは M0 のクレートに置き、CEF はトレイト `WebContentsHost` の実装として差し込む。
  P1 で `chromium-ffi` に差し替えても `shell-core` 以上は変更しない。
- `unsafe` を許すのは CEF 連携クレート(`cef-host`)のみ。それ以外は従来どおり `forbid`。
- メモリ最小化: セッション復元は最後に開いていたタブだけを生成し、残りは Discarded 状態で保持する
  (アクティブ化した時点で `restore` が実体を作る)。

## マイルストーン
| ID | 内容 | CEF 要否 | 完了条件 |
|---|---|---|---|
| P0-1 | core 拡張: `Committed`(URL 更新)/`RestoreTab`(破棄状態で復元) | 不要 | reducer テスト通過 |
| P0-2 | `shell-core`: オムニボックス送信、読込完了で履歴記録、ブックマーク切替、セッション保存/復元 | 不要 | cargo test 通過、80%+ |
| P0-3 | `cef-host`: `WebContentsHost` の CEF 実装、イベント(読込完了・タイトル・URL)を Shell へ通知 | 要 | CEF ダウンロード後にビルド成功 |
| P0-4 | `apps/browser`: ウィンドウ、アドレスバー、タブ操作、タイマーで Tick | 要 | 起動して実ページを表示 |
| P0-5 | 計測と G1 前段確認: 起動直後/1 ページ/20 タブのメモリを `bench/memory` の枠で記録 | 要 | 数値を記録(予算比較は P1) |

## リスク
- CEF 配布物のサイズ(数百 MB)とダウンロード。`cef` クレート(cef-rs)の `export-cef-dir` で共有配置する。
- CEF のウィンドウ/UI は Windows 実機での目視確認が必要。自動テストは `shell-core` までで担保する。
- P0 のメモリは CEF 由来の上乗せがあり、設計書の予算(150/250/600MB)は Content API 移行後(G2)で判定する。

## 実施結果(2026-10-26)
| ID | 状態 | 結果 |
|---|---|---|
| P0-1 | 完了 | `Committed` / `RestoreTab` を追加。reducer テスト 24 件通過 |
| P0-2 | 完了 | `crates/shell-core`(テスト 15 件、行カバレッジ約 90%)。`verify.ps1` 全段階通過 |
| P0-3 | 完了 | `apps/browser` の `CefHost`(Views の `BrowserView` をタブに対応付け)。clippy `-D warnings` 通過 |
| P0-4 | 完了 | 実機で起動・表示。アドレスバー検索、タブ列、履歴記録、終了時のセッション保存、再起動での復元を確認 |
| P0-5 | 一部 | 1 タブ(example.com)・debug ビルドで 7 プロセス、Private 合計 約 223MB(予算 250MB 以内)。20 タブの計測は未実施 |

### 実機で確認した内容
- `--url` 指定で起動し、タイトル `Example Domain - Rust Browser` が反映される(CEF → `shell-core` のイベント経路が動作)。
- ウィンドウを閉じると `history` に 1 件、`session` に 1 タブが保存され、プロセスが全て終了する。
- 再起動(`--url` なし)でタブが復元され、アドレスバーに `rust lang` + Enter で DuckDuckGo の検索結果に遷移する。

### 残課題(P1 以降)
- 凍結の実装(Content API)、フォーム内容の復元、サンドボックス有効化。
- ＋/×/タブ切替/☆ の実機操作の自動テスト、20 タブ時のメモリ計測。
- GPU プロセスの再起動ログ(この環境の AMD GPU)。ソフトウェア描画へのフォールバックで表示は問題ない。
- Windows の高 DPI でツールバーが大きく表示される(Views の既定スケール)。
