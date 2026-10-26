//! ネットワーク判断(設計書 §6 net-policy)。通信本体は Chromium の //net が行い、
//! ここは `ContentBrowserClient` のフック等から呼ばれる純粋な判断関数だけを持つ。

mod cookies;
mod decide;

pub use cookies::{cookie_decision, is_third_party, CookieDecision};
pub use decide::{decide, BlockReason, ContentBlocker, Decision, NoopBlocker, Request};
