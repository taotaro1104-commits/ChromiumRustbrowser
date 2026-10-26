//! シェルのロジック層: オムニボックス送信・履歴記録・ブックマーク・セッション保存/復元。
//! CEF などの実体には依存せず、`WebContentsHost` 越しに操作する(P0 設計 docs/P0-plan.md)。

mod session;
mod shell;

pub use session::MAX_SESSION_BYTES;
pub use shell::{Shell, ShellError};
