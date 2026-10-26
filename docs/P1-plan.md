# P1 実行計画(Content API 移行)

> **決定(2026-10-26): 前提は B(ビルド済みの成果物に限定する)。** Chromium の自前ビルドは行わず、CEF ベースで製品化する。
> このため下の「マイルストーン」(P1-0〜P1-6)は **保留** とし、実際の進め方は `docs/roadmap-cef.md` に移した。
> `crates/chromium-ffi` と `shim/rb_shim.h` は、将来ビルド環境が用意できた場合の足場として残す(削除しない)。
> 再開の条件: 32 コア / 64GB 級のビルド機(または分散ビルド)が用意できること。

目的(設計書 §10 フェーズ1 / ゲート G2): CEF を外し、Chromium の Content API を直接使うブラウザにする。
タブの凍結・破棄を実体に接続し、メモリ予算 4 シナリオ(起動直後 150MB / ページ 1 枚 250MB / タブ 20 枚 600MB / Rust 側 20MB)を CI で達成する。

## 現状(P0 の終わり)
- 判断ロジックはすべて `WebContentsHost` トレイトの向こう側にある(`browser-core` / `shell-core`)。CEF 実装は `apps/browser`。
- P1 で差し替えるのは **ホスト実装と UI だけ**。`shell-core` 以上は変えない方針で作ってある。
- 今回、差し替え先の最初の足場として `crates/chromium-ffi` を追加した:
  - `shim/rb_shim.h`: Rust と C++ shim の取り決め(関数表 `RbShimApi`。タブは u64 の ID、文字列は ptr+len)。
  - `ChromiumHost`: 関数表を呼ぶ `WebContentsHost`。戻り値コードを `HostError` に変換する。偽 shim によるテスト 2 件。
  - `unsafe` はこのクレートの `shim.rs` だけに閉じる。

## 前提(この環境では満たせない)
設計書の想定は 32 コア / 64GB メモリ / 分散ビルド。この環境は 8 論理コア / 14GB メモリ / 空き 240GB で、Chromium のフルビルドは
リンクでメモリ不足になり、所要時間も現実的でない。次のどちらかが必要:
1. ビルド専用機(32 コア / 64GB 以上、SSD 200GB 以上)または分散ビルド(reclient / siso)の用意。
2. ビルド済みの成果物に限定する。ただし Content API そのものを配布するビルド済み SDK は無く、現実の候補は CEF(今の P0 と同じ)か WebView2 になる。
   どちらも公開 API が Content API より狭く(凍結・プロセス配置・メモリ圧迫の制御などに届かない)、G1(Content API で起動)は満たせない。

## マイルストーン
| ID | 内容 | 要件 | 完了条件 |
|---|---|---|---|
| P1-0 | ビルド環境: depot_tools、`gclient sync`(固定したリリースタグ)、`gn gen`、`autoninja content_shell` | 上記の前提 | content_shell が起動する |
| P1-1 | C++ shim: `RbShimApi` を Content API で実装(`WebContents` の生成・`SetPageFrozen`・破棄・復元)。タブ ID ⇄ `WebContents` の表 | P1-0 | `chromium-ffi` の結合テスト(実 shim)が通る |
| P1-2 | `chromium-ffi` を cxx へ移行(イベント: タイトル/URL/読込完了/音声状態を Rust へ)。CEF 版 `apps/browser` と同じ `ctx` 経路に載せる | P1-1 | G1: Content API で起動しナビゲーション成功 |
| P1-3 | UI: Views(Chromium 内蔵)で現行 UI を再現 → 後続で Skia ネイティブ(`ui-native`)へ | P1-2 | 現行 UI と同じ操作ができる |
| P1-4 | 凍結・破棄の実接続: `SetPageFrozen`、`WebContents` 破棄、メモリ圧迫通知(`base::MemoryPressureListener`) | P1-2 | 実機で 5 分凍結 / 30 分破棄が動く |
| P1-5 | ページ状態: スクロール・入力中・WebRTC 使用中のフラグを `RenderFrameHost` / `WebContentsObserver` から取得(console 経由をやめる) | P1-2 | 保護ルールが実イベントで働く |
| P1-6 | `bench/memory` に Chromium 計測を追加(プロセス別 Private)、CI で 4 シナリオを判定 | P1-4 | G2: ドッグフード開始 + 予算を CI で達成 |

## P0 で分かったことのうち P1 に効くもの
- 破棄は **実体のプロセスが終わるまで時間がかかる**(レンダラーは 1 つ / 約 10 秒ずつ減る)。メモリ判定は破棄後 3 分以上待つこと。
- CEF の Views では、タブの BrowserView を閉じるとウィンドウごと閉じようとされる。Content API ではこの制約は無い想定だが、
  UI を Views で組む間は同じ注意が要る(`apps/browser/src/host.rs` の `CLOSING`)。
- サンドボックスは CEF では `bootstrap.exe` 方式で有効にした。Content API 直なら標準の仕組みで有効になる。
- Windows で GPU プロセスが落ち続ける環境がある(AMD)。P1-0 で ANGLE/ソフトウェアの切替を確認しておく。

## 判断が要る点
- P1-0 の前提(専用ビルド機 or ビルド済み成果物)をどちらにするか。これが決まるまで P1-1 以降は着手しない。
- 上記が難しい間は、P0 の CEF 版を使い続けながら `shell-core` 側(ページ状態の保護ルール、メモリ計測)を深める。
