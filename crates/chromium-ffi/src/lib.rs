//! Chromium(Content API)実装の `WebContentsHost`。C++ shim の関数表(`shim/rb_shim.h`)を呼ぶだけの
//! 薄い層で、判断ロジックは持たない。P1 で shim を実装するまでは、テスト用の偽 shim で契約を検証する。

mod shim;

pub use shim::{ChromiumHost, ShimApi, RB_FAILED, RB_OK, RB_UNKNOWN_TAB};
