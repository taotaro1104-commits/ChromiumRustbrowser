//! パスワード管理のロジック。保存先は OS の資格情報ストア(`SecretStore` 越し)で、DB には「どのサイトに
//! どのユーザー名があるか」の索引だけを置く(DB が漏れてもパスワードは出ない)。
//!
//! 方針:
//! - サイトの単位は **オリジン**(スキーム + ホスト + ポート)。https のみ。http は開発用のループバックだけ。
//!   別ポート・サブドメイン・http は別サイトとして扱い、保存済みの情報を渡さない(フィッシング対策)。
//! - 入力・返却するパスワードは `Zeroizing` で、使い終わると上書きされる。ログやエラー文に含めない。
//! - 自動入力の可否は呼び出し側(UI)が利用者の操作を経て決める。このクレートは勝手に渡さない。

mod origin;
mod secret;
mod vault;

pub use origin::normalize_origin;
pub use secret::{MemorySecretStore, SecretError, SecretStore};
pub use vault::{
    PromptKind, SaveOutcome, Vault, VaultError, MAX_PASSWORD_BYTES, MAX_USERNAME_BYTES,
};
pub use zeroize::Zeroizing;
