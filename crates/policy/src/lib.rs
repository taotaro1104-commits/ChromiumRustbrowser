//! 企業ポリシーの型付き読み込み(設計書 §6 policy)。
//! 外部入力は構造体へパースしてから使う("parse, don't validate")。
//! GPO / plist ローダーはフェーズ3で `PolicySource` を実装して追加する。

mod host_pattern;
mod model;

pub use host_pattern::{HostPattern, HostSet};
pub use model::{
    CookiePolicy, JsonSource, Policy, PolicyError, PolicySource, SiteIsolation, MAX_POLICY_BYTES,
};
