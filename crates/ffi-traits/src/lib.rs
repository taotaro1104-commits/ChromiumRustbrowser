//! Chromium 機能を表す抽象トレイト群。
//!
//! C++ の生ポインタは一切ここに出さず、不透明ハンドルだけを渡す(設計書 §5 規約1)。
//! 実体は `chromium-ffi`(フェーズ1)か CEF 実装が提供し、テストでは `mock` を使う。

mod host;
#[cfg(feature = "mock")]
pub mod mock;

pub use host::{Clock, HostError, MemoryPressure, TabHandle, WebContentsHost};
