# ロードマップ(CEF ベース)

決定(2026-10-26): Chromium の自前ビルドは行わず、**CEF のビルド済み配布物の上で製品化する**(docs/P1-plan.md の選択肢 B)。
`WebContentsHost` で差し替えられる設計は維持するので、将来ビルド環境が用意できれば `crates/chromium-ffi` から Content API へ移れる。

## 設計書との差分
| 項目 | 設計書(Content API 前提) | CEF ベースでの扱い |
|---|---|---|
| G1「Content API で起動」 | 必須 | **CEF で起動・ナビゲーション成功**に読み替える(P0 で達成済み) |
| ページ凍結(5 分) | `SetPageFrozen` | 公開 API が無いため**実装しない**。非表示+破棄(30 分)で代替 |
| 破棄(30 分 / メモリ圧迫) | `WebContents` 破棄 | 実装済み(`close_browser`)。実測で効果を確認 |
| ページ状態の取得 | `WebContentsObserver` | JS + console 経由(暫定だが製品でも使う) |
| メモリ予算 | 150 / 250 / 600 / 20 MB | **CEF 基準で引き直す**(下記の実測)。Rust 側 20MB は据え置き |
| PartitionAlloc、プロセス配置の制御 | 目標に含む | 対象外 |
| G2「ドッグフード開始 + 予算を CI で達成」 | P1 末 | CEF 基準の予算で判定する |

## メモリ予算の再設定
**実測で確定**(2026-10-26、release ビルド、空きメモリ 5GB 超の状態)。予算 = 実測 + 約 15% の余裕を丸めた値。
判定は Private メモリの全プロセス合計。CI では中央値で判定し、前回比 +5% で失敗(`bench/memory`)。

| シナリオ | 設計書(Content API 目標) | 実測(CEF, release) | 予算 |
|---|---|---|---|
| 起動直後(スタートページ 1 枚) | 150MB | 220MB | **260MB** |
| ページ 1 枚(example.com) | 250MB | 266MB | **310MB** |
| タブ 20 枚(別サイトの実サイト) | 600MB | 1,592MB | **1,850MB** |
| 〃 を破棄した約 90 秒後(参考) | — | 396MB | (予算化は自動計測の後) |
| Rust 側 | 20MB | 約 7MB | **20MB**(据え置き) |

- 20 タブの内訳: example.com/org/net、wikipedia、github、rust-lang、duckduckgo、mozilla、python、w3、iana、openstreetmap、archive、gnu、
  kernel、debian、apache、eff、ietf、スタートページ。プロセスは 39 個(うちレンダラー約 30)。値はサイトの重さに強く依存する。
- 破棄(メモリ圧迫と同じ動作)で **1,592MB → 396MB(約 75% 減)**、プロセスは 39 → 7。レンダラーが順に終了するため、約 90 秒かかる。
  修正前(`do_close` 未対応)はほとんど減らなかった。
- 以前の参考値(起動 219〜242 / 1 枚 268〜289 / 20 タブ約 1,660)と今回の実測は ±10% 以内で一致した。
- `memory-bench` で 3 つの CEF シナリオも自動判定できる(`--attach`)。**状態を作るのは呼び出し側**で、起動中の
  `rustbrowser` の全プロセスの Private メモリ合計を間隔を空けて測り、中央値で予算・前回比を判定する。

```
# 例: 20 タブを開いた状態で測る(--samples 3 --interval-secs 5 が既定)
cargo run -p memory-bench -- --attach rustbrowser --scenario tabs_20 --baseline bench/memory/baseline.txt
# 判定が通ったら前回値を更新(測っていないシナリオの値は残る)
cargo run -p memory-bench -- --attach rustbrowser --scenario tabs_20 --baseline bench/memory/baseline.txt --write-baseline
```

  シナリオ名は `startup` / `single_page` / `tabs_20`。終了コードは、通過 0、予算超過または前回比 +5% 超で 1、引数・計測の失敗で 2。
  PowerShell の `Get-Process` の Private 合計と一致することを実機で確認(起動直後 215MB / 6 プロセス)。
  タブを開く操作の自動化(UI 操作)はまだ無く、CI へ入れるには別途ドライバが要る。

## 次の作業(優先順)
1. **配布の土台**: `bootstrap.exe` 構成のパッケージ化(署名、インストーラ、CEF 更新手順)。
2. **メモリ**: 破棄の時間設定(30 分)の見直し、タブ数に応じた自動破棄、メモリ圧迫の実検知(Windows のメモリ通知)。
3. **P2 の機能**: ダウンロード、パスワード管理(OS の資格情報ストア)、広告ブロック(adblock-rust を `net-policy` の `Blocker` 口に接続)、
   アップデータ(CEF の更新を含む)、クラッシュレポート。
4. **P3**: 同期(E2E 暗号)、拡張機能(CEF の Alloy スタイルでは対応が限られるため、対象 API を絞って再評価)。
